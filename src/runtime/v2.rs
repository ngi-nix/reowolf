use crate::common::*;
use crate::runtime::endpoint::Endpoint;
use crate::runtime::endpoint::Msg;
use crate::runtime::ProtocolD;
use crate::runtime::ProtocolS;
use std::io::Write;

#[derive(Default)]
struct IntStream {
    next: u32,
}
struct IdManager {
    controller_id: ControllerId,
    port_suffix_stream: IntStream,
}

struct ProtoComponent {
    state: ProtocolS,
    ports: HashSet<PortId>,
}
enum InpRoute {
    NativeComponent,
    ProtoComponent { index: usize },
    Endpoint { index: usize },
}
trait Logger {
    fn line_writer(&mut self) -> &mut dyn Write;
}
#[derive(Clone)]
struct EndpointSetup {
    polarity: Polarity,
    sock_addr: SocketAddr,
    is_active: bool,
}
struct EndpointExt {
    net_endpoint: Endpoint,
    // data-messages emerging from this endpoint are destined for this inp
    inp: Port,
}
struct Neighborhood {
    parent: Option<usize>,
    children: Vec<usize>, // ordered, deduplicated
}
struct MemInMsg {
    inp: Port,
    msg: Payload,
}
struct EndpointPoller {
    poll: Poll,
    events: Events,
    undrained_endpoints: HashSet<usize>,
    delayed_inp_messages: Vec<(Port, Msg)>,
}
struct Connector {
    logger: Box<dyn Logger>,
    proto_description: Arc<ProtocolD>,
    id_manager: IdManager,
    native_ports: HashSet<PortId>,
    proto_components: Vec<ProtoComponent>,
    outp_to_inp: HashMap<PortId, PortId>,
    inp_to_route: HashMap<PortId, InpRoute>,
    phased: ConnectorPhased,
}
enum ConnectorPhased {
    Setup {
        endpoint_setups: Vec<(PortId, EndpointSetup)>,
        surplus_sockets: u16,
    },
    Communication {
        endpoint_poller: EndpointPoller,
        endpoint_exts: Vec<EndpointExt>,
        neighborhood: Neighborhood,
        mem_inbox: Vec<MemInMsg>,
    },
}
/////////////////////////////
impl IntStream {
    fn next(&mut self) -> u32 {
        if self.next == u32::MAX {
            panic!("NO NEXT!")
        }
        self.next += 1;
        self.next - 1
    }
}
impl IdManager {
    fn next_port(&mut self) -> PortId {
        let port_suffix = self.port_suffix_stream.next();
        let controller_id = self.controller_id;
        PortId { controller_id, port_index: port_suffix }
    }
    fn new(controller_id: ControllerId) -> Self {
        Self { controller_id, port_suffix_stream: Default::default() }
    }
}
impl Connector {
    pub fn new(
        logger: Box<dyn Logger>,
        proto_description: Arc<ProtocolD>,
        controller_id: ControllerId,
        surplus_sockets: u16,
    ) -> Self {
        Self {
            logger,
            proto_description,
            id_manager: IdManager::new(controller_id),
            native_ports: Default::default(),
            proto_components: Default::default(),
            outp_to_inp: Default::default(),
            inp_to_route: Default::default(),
            phased: ConnectorPhased::Setup { endpoint_setups: Default::default(), surplus_sockets },
        }
    }
    pub fn add_port_pair(&mut self) -> [PortId; 2] {
        let o = self.id_manager.next_port();
        let i = self.id_manager.next_port();
        self.outp_to_inp.insert(o, i);
        self.native_ports.insert(o);
        self.native_ports.insert(i);
        [o, i]
    }
    pub fn add_net_port(&mut self, endpoint_setup: EndpointSetup) -> Result<PortId, ()> {
        match &mut self.phased {
            ConnectorPhased::Setup { endpoint_setups, .. } => {
                let p = self.id_manager.next_port();
                endpoint_setups.push((p, endpoint_setup));
                Ok(p)
            }
            ConnectorPhased::Communication { .. } => Err(()),
        }
    }
    fn check_polarity(&self, port: &PortId) -> Polarity {
        if self.outp_to_inp.contains_key(port) {
            Polarity::Putter
        } else {
            assert!(self.inp_to_route.contains_key(port));
            Polarity::Getter
        }
    }
    pub fn add_proto_component(&mut self, identifier: &[u8], ports: &[PortId]) -> Result<(), ()> {
        let polarities = self.proto_description.component_polarities(identifier).map_err(drop)?;
        if polarities.len() != ports.len() {
            return Err(());
        }
        for (&expected_polarity, port) in polarities.iter().zip(ports.iter()) {
            if !self.native_ports.contains(port) {
                return Err(());
            }
            if expected_polarity != self.check_polarity(port) {
                return Err(());
            }
        }
        // ok!
        let state = self.proto_description.new_main_component(identifier, ports);
        let proto_component = ProtoComponent { ports: ports.iter().copied().collect(), state };
        let proto_component_index = self.proto_components.len();
        self.proto_components.push(proto_component);
        for port in ports.iter() {
            if let Polarity::Getter = self.check_polarity(port) {
                self.inp_to_route
                    .insert(*port, InpRoute::ProtoComponent { index: proto_component_index });
            }
        }
        Ok(())
    }
    pub fn connect(&mut self, timeout: Duration) -> Result<(), ()> {
        match &mut self.phased {
            ConnectorPhased::Communication { .. } => Err(()),
            ConnectorPhased::Setup { endpoint_setups, surplus_sockets } => {
                // connect all endpoints in parallel; send and receive peer ids through ports
                let (mut endpoint_exts, mut endpoint_poller) =
                    init_endpoints(endpoint_setups, timeout)?;
                write!(
                    self.logger.line_writer(),
                    "hello! I am controller_id:{}",
                    self.id_manager.controller_id
                );
                // leader election and tree construction
                let neighborhood = init_neighborhood(&mut endpoint_exts, &mut endpoint_poller)?;
                // TODO session optimization goes here
                self.phased = ConnectorPhased::Communication {
                    endpoint_poller,
                    endpoint_exts,
                    neighborhood,
                    mem_inbox: Default::default(),
                };
                Ok(())
            }
        }
    }
}

fn init_endpoints(
    endpoint_setups: &[(PortId, EndpointSetup)],
    timeout: Duration,
) -> Result<(Vec<EndpointExt>, EndpointPoller), ()> {
    let mut endpoint_poller = EndpointPoller {
        poll: Poll::new().map_err(drop)?,
        events: Events::with_capacity(64),
        undrained_endpoints: Default::default(),
        delayed_inp_messages: Default::default(),
    };
    const PORT_ID_LEN: usize = std::mem::size_of::<PortId>();
    enum MaybeRecvPort {
        Complete(Port),
        Partial { buf: [u8; PORT_ID_LEN], read: u8 },
    }
    struct Todo {
        endpoint: TodoEndpoint,
        polarity: Polarity,
        local_port: Port,
        sent_local_port: bool,
        recv_peer_port: MaybeRecvPort,
    }
    enum TodoEndpoint {
        Listener(mio::net::TcpListener),
        Stream(mio::net::TcpStream),
    }
    const BOTH: mio::Interest = mio::Interest::READABLE.add(mio::Interest::WRITABLE);
    fn init(
        token: Token,
        local_port: Port,
        endpoint_setup: &EndpointSetup,
        poll: &mut Poll,
    ) -> Result<Todo, ()> {
        let endpoint = if endpoint_setup.is_active {
            let mut stream =
                mio::net::TcpStream::connect(&endpoint_setup.sock_addr).map_err(drop)?;
            poll.registry().register(&mut stream, token, BOTH).unwrap();
            TodoEndpoint::Stream(stream)
        } else {
            let mut listener =
                mio::net::TcpListener::bind(&endpoint_setup.sock_addr).map_err(drop)?;
            poll.registry().register(&mut listener, token, BOTH).unwrap();
            TodoEndpoint::Listener(listener)
        };
        Ok(Todo {
            endpoint,
            endpoint_setup: endpoint_setup.clone(),
            local_port,
            sent_local_port: false,
            recv_peer_port: MaybeRecvPort::Partial { buf: [0; 8], read: 0 },
        })
    };

    let todos = endpoint_setups
        .iter()
        .enumerate()
        .map(|(index, (local_port, endpoint_setup))| {
            init(Token(index), local_port, endpoint_setup, &mut endpoint_poller.poll)
        })
        .collect::<Result<Vec<Todo>, _>>()?;
    let endpoint_exts = vec![];
    Ok((endpoint_exts, endpoint_poller))
}

fn init_neighborhood(
    endpoint_exts: &mut [EndpointExt],
    endpoint_poller: &mut EndpointPoller,
) -> Result<Neighborhood, ()> {
    todo!()
}
