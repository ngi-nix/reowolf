mod communication;
mod endpoints;
pub mod error;
mod setup;

#[cfg(test)]
mod tests;

use crate::common::*;
use error::*;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum LocalComponentId {
    Native,
    Proto(ProtoComponentId),
}
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Route {
    LocalComponent(LocalComponentId),
    Endpoint { index: usize },
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MyPortInfo {
    polarity: Polarity,
    port: PortId,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Decision {
    Failure,
    Success(Predicate),
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Msg {
    SetupMsg(SetupMsg),
    CommMsg(CommMsg),
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum SetupMsg {
    MyPortInfo(MyPortInfo),
    LeaderEcho { maybe_leader: ControllerId },
    LeaderAnnounce { leader: ControllerId },
    YouAreMyParent,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CommMsg {
    pub round_index: usize,
    pub contents: CommMsgContents,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum CommMsgContents {
    SendPayload(SendPayloadMsg),
    Suggest { suggestion: Decision }, // SINKWARD
    Announce { decision: Decision },  // SINKAWAYS
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SendPayloadMsg {
    predicate: Predicate,
    payload: Payload,
}
#[derive(Debug, PartialEq)]
pub enum CommonSatResult {
    FormerNotLatter,
    LatterNotFormer,
    Equivalent,
    New(Predicate),
    Nonexistant,
}
pub struct Endpoint {
    inbox: Vec<u8>,
    stream: TcpStream,
}
#[derive(Debug, Clone)]
pub struct ProtoComponent {
    state: ComponentState,
    ports: HashSet<PortId>,
}
pub trait Logger: Debug {
    fn line_writer(&mut self) -> &mut dyn std::io::Write;
}
#[derive(Debug)]
pub struct VecLogger(ControllerId, Vec<u8>);
#[derive(Debug)]
pub struct DummyLogger;
#[derive(Debug)]
pub struct FileLogger(ControllerId, std::fs::File);
#[derive(Debug, Clone)]
pub struct EndpointSetup {
    pub sock_addr: SocketAddr,
    pub is_active: bool,
}
#[derive(Debug)]
pub struct EndpointExt {
    endpoint: Endpoint,
    getter_for_incoming: PortId,
}
#[derive(Debug)]
pub struct Neighborhood {
    parent: Option<usize>,
    children: Vec<usize>, // ordered, deduplicated
}
#[derive(Debug)]
pub struct MemInMsg {
    inp: PortId,
    msg: Payload,
}
#[derive(Debug)]
pub struct IdManager {
    controller_id: ControllerId,
    port_suffix_stream: U32Stream,
    proto_component_suffix_stream: U32Stream,
}
#[derive(Debug)]
pub struct EndpointManager {
    // invariants:
    // 1. endpoint N is registered READ | WRITE with poller
    // 2. Events is empty
    poll: Poll,
    events: Events,
    polled_undrained: IndexSet<usize>,
    delayed_messages: Vec<(usize, Msg)>,
    undelayed_messages: Vec<(usize, Msg)>,
    endpoint_exts: Vec<EndpointExt>,
}
#[derive(Debug, Default)]
pub struct PortInfo {
    polarities: HashMap<PortId, Polarity>,
    peers: HashMap<PortId, PortId>,
    routes: HashMap<PortId, Route>,
}
#[derive(Debug)]
pub struct Connector {
    proto_description: Arc<ProtocolDescription>,
    proto_components: HashMap<ProtoComponentId, ProtoComponent>,
    logger: Box<dyn Logger>,
    id_manager: IdManager,
    native_ports: HashSet<PortId>,
    port_info: PortInfo,
    phased: ConnectorPhased,
}
#[derive(Debug)]
pub enum ConnectorPhased {
    Setup {
        endpoint_setups: Vec<(PortId, EndpointSetup)>,
        surplus_sockets: u16,
    },
    Communication {
        round_index: usize,
        endpoint_manager: EndpointManager,
        neighborhood: Neighborhood,
        mem_inbox: Vec<MemInMsg>,
        native_batches: Vec<NativeBatch>,
        round_result: Result<Option<(usize, HashMap<PortId, Payload>)>, SyncError>,
    },
}
#[derive(Default, Clone, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Predicate {
    pub assigned: BTreeMap<FiringVar, bool>,
}
#[derive(Debug, Default)]
pub struct NativeBatch {
    // invariant: putters' and getters' polarities respected
    to_put: HashMap<PortId, Payload>,
    to_get: HashSet<PortId>,
}
pub struct NonsyncProtoContext<'a> {
    logger: &'a mut dyn Logger,
    proto_component_id: ProtoComponentId,
    port_info: &'a mut PortInfo,
    id_manager: &'a mut IdManager,
    proto_component_ports: &'a mut HashSet<PortId>,
    unrun_components: &'a mut Vec<(ProtoComponentId, ProtoComponent)>,
}
pub struct SyncProtoContext<'a> {
    logger: &'a mut dyn Logger,
    predicate: &'a Predicate,
    port_info: &'a PortInfo,
    inbox: &'a HashMap<PortId, Payload>,
}
////////////////
impl PortInfo {
    fn firing_var_for(&self, port: PortId) -> FiringVar {
        FiringVar(match self.polarities.get(&port).unwrap() {
            Getter => port,
            Putter => *self.peers.get(&port).unwrap(),
        })
    }
}
impl IdManager {
    fn new(controller_id: ControllerId) -> Self {
        Self {
            controller_id,
            port_suffix_stream: Default::default(),
            proto_component_suffix_stream: Default::default(),
        }
    }
    fn new_port_id(&mut self) -> PortId {
        Id { controller_id: self.controller_id, u32_suffix: self.port_suffix_stream.next() }.into()
    }
    fn new_proto_component_id(&mut self) -> ProtoComponentId {
        Id {
            controller_id: self.controller_id,
            u32_suffix: self.proto_component_suffix_stream.next(),
        }
        .into()
    }
}
impl Drop for Connector {
    fn drop(&mut self) {
        log!(&mut *self.logger, "Connector dropping. Goodbye!");
    }
}
impl Connector {
    pub fn swap_logger(&mut self, mut new_logger: Box<dyn Logger>) -> Box<dyn Logger> {
        std::mem::swap(&mut self.logger, &mut new_logger);
        new_logger
    }
    pub fn get_logger(&mut self) -> &mut dyn Logger {
        &mut *self.logger
    }
    pub fn new_port_pair(&mut self) -> [PortId; 2] {
        // adds two new associated ports, related to each other, and exposed to the native
        let [o, i] = [self.id_manager.new_port_id(), self.id_manager.new_port_id()];
        self.native_ports.insert(o);
        self.native_ports.insert(i);
        // {polarity, peer, route} known. {} unknown.
        self.port_info.polarities.insert(o, Putter);
        self.port_info.polarities.insert(i, Getter);
        self.port_info.peers.insert(o, i);
        self.port_info.peers.insert(i, o);
        let route = Route::LocalComponent(LocalComponentId::Native);
        self.port_info.routes.insert(o, route);
        self.port_info.routes.insert(i, route);
        log!(self.logger, "Added port pair (out->in) {:?} -> {:?}", o, i);
        [o, i]
    }
    pub fn add_component(
        &mut self,
        identifier: &[u8],
        ports: &[PortId],
    ) -> Result<(), AddComponentError> {
        // called by the USER. moves ports owned by the NATIVE
        use AddComponentError::*;
        // 1. check if this is OK
        let polarities = self.proto_description.component_polarities(identifier)?;
        if polarities.len() != ports.len() {
            return Err(WrongNumberOfParamaters { expected: polarities.len() });
        }
        for (&expected_polarity, port) in polarities.iter().zip(ports.iter()) {
            if !self.native_ports.contains(port) {
                return Err(UnknownPort(*port));
            }
            if expected_polarity != *self.port_info.polarities.get(port).unwrap() {
                return Err(WrongPortPolarity { port: *port, expected_polarity });
            }
        }
        // 3. remove ports from old component & update port->route
        let new_id = self.id_manager.new_proto_component_id();
        for port in ports.iter() {
            self.port_info
                .routes
                .insert(*port, Route::LocalComponent(LocalComponentId::Proto(new_id)));
        }
        self.native_ports.retain(|port| !ports.contains(port));
        // 4. add new component
        self.proto_components.insert(
            new_id,
            ProtoComponent {
                state: self.proto_description.new_main_component(identifier, ports),
                ports: ports.iter().copied().collect(),
            },
        );
        Ok(())
    }
}
impl Logger for DummyLogger {
    fn line_writer(&mut self) -> &mut dyn std::io::Write {
        impl std::io::Write for DummyLogger {
            fn flush(&mut self) -> Result<(), std::io::Error> {
                Ok(())
            }
            fn write(&mut self, bytes: &[u8]) -> Result<usize, std::io::Error> {
                Ok(bytes.len())
            }
        }
        self
    }
}
impl VecLogger {
    pub fn new(controller_id: ControllerId) -> Self {
        Self(controller_id, Default::default())
    }
}
impl Drop for VecLogger {
    fn drop(&mut self) {
        let stdout = std::io::stderr();
        let mut lock = stdout.lock();
        writeln!(lock, "--- DROP LOG DUMP ---").unwrap();
        let _ = std::io::Write::write(&mut lock, self.1.as_slice());
    }
}
impl Logger for VecLogger {
    fn line_writer(&mut self) -> &mut dyn std::io::Write {
        let _ = write!(&mut self.1, "\nCID({}): ", self.0);
        self
    }
}
impl FileLogger {
    pub fn new(controller_id: ControllerId, file: std::fs::File) -> Self {
        Self(controller_id, file)
    }
}
impl Logger for FileLogger {
    fn line_writer(&mut self) -> &mut dyn std::io::Write {
        let _ = write!(&mut self.1, "\nCID({}): ", self.0);
        &mut self.1
    }
}
impl std::io::Write for VecLogger {
    fn flush(&mut self) -> Result<(), std::io::Error> {
        Ok(())
    }
    fn write(&mut self, data: &[u8]) -> Result<usize, std::io::Error> {
        self.1.extend_from_slice(data);
        Ok(data.len())
    }
}
impl Predicate {
    #[inline]
    pub fn inserted(mut self, k: FiringVar, v: bool) -> Self {
        self.assigned.insert(k, v);
        self
    }
    // returns true IFF self.unify would return Equivalent OR FormerNotLatter
    pub fn satisfies(&self, other: &Self) -> bool {
        let mut s_it = self.assigned.iter();
        let mut s = if let Some(s) = s_it.next() {
            s
        } else {
            return other.assigned.is_empty();
        };
        for (oid, ob) in other.assigned.iter() {
            while s.0 < oid {
                s = if let Some(s) = s_it.next() {
                    s
                } else {
                    return false;
                };
            }
            if s.0 > oid || s.1 != ob {
                return false;
            }
        }
        true
    }

    /// Given self and other, two predicates, return the most general Predicate possible, N
    /// such that n.satisfies(self) && n.satisfies(other).
    /// If none exists Nonexistant is returned.
    /// If the resulting predicate is equivlanet to self, other, or both,
    /// FormerNotLatter, LatterNotFormer and Equivalent are returned respectively.
    /// otherwise New(N) is returned.
    pub fn common_satisfier(&self, other: &Self) -> CommonSatResult {
        use CommonSatResult as Csr;
        // iterators over assignments of both predicates. Rely on SORTED ordering of BTreeMap's keys.
        let [mut s_it, mut o_it] = [self.assigned.iter(), other.assigned.iter()];
        let [mut s, mut o] = [s_it.next(), o_it.next()];
        // lists of assignments in self but not other and vice versa.
        let [mut s_not_o, mut o_not_s] = [vec![], vec![]];
        loop {
            match [s, o] {
                [None, None] => break,
                [None, Some(x)] => {
                    o_not_s.push(x);
                    o_not_s.extend(o_it);
                    break;
                }
                [Some(x), None] => {
                    s_not_o.push(x);
                    s_not_o.extend(s_it);
                    break;
                }
                [Some((sid, sb)), Some((oid, ob))] => {
                    if sid < oid {
                        // o is missing this element
                        s_not_o.push((sid, sb));
                        s = s_it.next();
                    } else if sid > oid {
                        // s is missing this element
                        o_not_s.push((oid, ob));
                        o = o_it.next();
                    } else if sb != ob {
                        assert_eq!(sid, oid);
                        // both predicates assign the variable but differ on the value
                        return Csr::Nonexistant;
                    } else {
                        // both predicates assign the variable to the same value
                        s = s_it.next();
                        o = o_it.next();
                    }
                }
            }
        }
        // Observed zero inconsistencies. A unified predicate exists...
        match [s_not_o.is_empty(), o_not_s.is_empty()] {
            [true, true] => Csr::Equivalent,       // ... equivalent to both.
            [false, true] => Csr::FormerNotLatter, // ... equivalent to self.
            [true, false] => Csr::LatterNotFormer, // ... equivalent to other.
            [false, false] => {
                // ... which is the union of the predicates' assignments but
                //     is equivalent to neither self nor other.
                let mut new = self.clone();
                for (&id, &b) in o_not_s {
                    new.assigned.insert(id, b);
                }
                Csr::New(new)
            }
        }
    }
    pub fn union_with(&self, other: &Self) -> Option<Self> {
        let mut res = self.clone();
        for (&channel_id, &assignment_1) in other.assigned.iter() {
            match res.assigned.insert(channel_id, assignment_1) {
                Some(assignment_2) if assignment_1 != assignment_2 => return None,
                _ => {}
            }
        }
        Some(res)
    }
    pub fn query(&self, var: FiringVar) -> Option<bool> {
        self.assigned.get(&var).copied()
    }
}
impl Debug for Predicate {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        struct MySet<'a>(&'a Predicate, bool);
        impl Debug for MySet<'_> {
            fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
                let iter = self.0.assigned.iter().filter_map(|(port, &firing)| {
                    if firing == self.1 {
                        Some(port)
                    } else {
                        None
                    }
                });
                f.debug_set().entries(iter).finish()
            }
        }
        f.debug_struct("Predicate")
            .field("Trues", &MySet(self, true))
            .field("Falses", &MySet(self, false))
            .finish()
    }
}
