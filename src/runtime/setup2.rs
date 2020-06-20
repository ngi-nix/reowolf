use crate::common::*;
use crate::runtime::*;

struct LogicalChannelInfo {
    local_port: PortId,
    peer_port: PortId,
    local_polarity: Polarity,
    endpoint_index: usize,
}
///////////////
impl Connector {
    pub fn new_simple(
        proto_description: Arc<ProtocolDescription>,
        controller_id: ControllerId,
    ) -> Self {
        let logger = Box::new(StringLogger::new(controller_id));
        let surplus_sockets = 8;
        Self::new(logger, proto_description, controller_id, surplus_sockets)
    }
    pub fn new(
        logger: Box<dyn Logger>,
        proto_description: Arc<ProtocolDescription>,
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
        self.inp_to_route.insert(i, InpRoute::NativeComponent);
        self.native_ports.insert(o);
        self.native_ports.insert(i);
        log!(self.logger, "Added port pair (out->in) {:?} -> {:?}", o, i);
        [o, i]
    }
    pub fn add_net_port(&mut self, endpoint_setup: EndpointSetup) -> Result<PortId, ()> {
        match &mut self.phased {
            ConnectorPhased::Setup { endpoint_setups, .. } => {
                let p = self.id_manager.next_port();
                self.native_ports.insert(p);
                if endpoint_setup.polarity == Getter {
                    self.inp_to_route.insert(p, InpRoute::NativeComponent);
                }
                log!(self.logger, "Added net port {:?} with info {:?} ", p, &endpoint_setup);
                endpoint_setups.push((p, endpoint_setup));
                Ok(p)
            }
            ConnectorPhased::Communication { .. } => Err(()),
        }
    }
    fn check_polarity(&self, port: &PortId) -> Polarity {
        if let ConnectorPhased::Setup { endpoint_setups, .. } = &self.phased {
            for (setup_port, EndpointSetup { polarity, .. }) in endpoint_setups.iter() {
                if setup_port == port {
                    // special case. this port's polarity isn't reflected by
                    // self.inp_to_route or self.outp_to_inp, because its still not paired to a peer
                    return *polarity;
                }
            }
        }
        if self.outp_to_inp.contains_key(port) {
            Polarity::Putter
        } else {
            assert!(self.inp_to_route.contains_key(port));
            Polarity::Getter
        }
    }
    pub fn add_component(
        &mut self,
        identifier: &[u8],
        ports: &[PortId],
    ) -> Result<(), AddComponentError> {
        use AddComponentError::*;
        let polarities = self.proto_description.component_polarities(identifier)?;
        if polarities.len() != ports.len() {
            return Err(WrongNumberOfParamaters { expected: polarities.len() });
        }
        for (&expected_polarity, port) in polarities.iter().zip(ports.iter()) {
            if !self.native_ports.contains(port) {
                return Err(UnknownPort(*port));
            }
            if expected_polarity != self.check_polarity(port) {
                return Err(WrongPortPolarity { port: *port, expected_polarity });
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
            ConnectorPhased::Communication { .. } => {
                log!(self.logger, "Call to connecting in connected state");
                Err(())
            }
            ConnectorPhased::Setup { endpoint_setups, .. } => {
                log!(self.logger, "Call to connecting in setup state. Timeout {:?}", timeout);
                let deadline = Instant::now() + timeout;
                // connect all endpoints in parallel; send and receive peer ids through ports
                let mut endpoint_manager = {
                    let Self { outp_to_inp, inp_to_route, logger, .. } = self;
                    let logical_channel_callback = |lci: LogicalChannelInfo| {
                        if let Putter = lci.local_polarity {
                            outp_to_inp.insert(lci.local_port, lci.peer_port);
                            inp_to_route.insert(
                                lci.peer_port,
                                InpRoute::Endpoint { index: lci.endpoint_index },
                            );
                        }
                    };
                    new_endpoint_manager(
                        &mut **logger,
                        endpoint_setups,
                        logical_channel_callback,
                        deadline,
                    )?
                };
                log!(
                    self.logger,
                    "Successfully connected {} endpoints",
                    endpoint_manager.endpoint_exts.len()
                );
                // leader election and tree construction
                let neighborhood = init_neighborhood(
                    self.id_manager.controller_id,
                    &mut *self.logger,
                    &mut endpoint_manager,
                    deadline,
                )?;
                log!(self.logger, "Successfully created neighborhood {:?}", &neighborhood);
                // TODO session optimization goes here
                self.phased = ConnectorPhased::Communication {
                    endpoint_manager,
                    neighborhood,
                    mem_inbox: Default::default(),
                };
                Ok(())
            }
        }
    }
}

fn new_endpoint_manager(
    logger: &mut dyn Logger,
    endpoint_setups: &[(PortId, EndpointSetup)],
    mut logical_channel_callback: impl FnMut(LogicalChannelInfo),
    deadline: Instant,
) -> Result<EndpointManager, ()> {
    ////////////////////////////////////////////
    const BOTH: Interest = Interest::READABLE.add(Interest::WRITABLE);
    struct Todo {
        todo_endpoint: TodoEndpoint,
        endpoint_setup: EndpointSetup,
        local_port: PortId,
        sent_local_port: bool,          // true <-> I've sent my local port
        recv_peer_port: Option<PortId>, // Some(..) <-> I've received my peer's port
    }
    enum TodoEndpoint {
        Listener(TcpListener),
        Endpoint(Endpoint),
    }
    fn init_todo(
        token: Token,
        local_port: PortId,
        endpoint_setup: &EndpointSetup,
        poll: &mut Poll,
    ) -> Result<Todo, ()> {
        let todo_endpoint = if endpoint_setup.is_active {
            let mut stream = TcpStream::connect(endpoint_setup.sock_addr).map_err(drop)?;
            poll.registry().register(&mut stream, token, BOTH).unwrap();
            TodoEndpoint::Endpoint(Endpoint { stream, inbox: vec![] })
        } else {
            let mut listener = TcpListener::bind(endpoint_setup.sock_addr).map_err(drop)?;
            poll.registry().register(&mut listener, token, BOTH).unwrap();
            TodoEndpoint::Listener(listener)
        };
        Ok(Todo {
            todo_endpoint,
            endpoint_setup: endpoint_setup.clone(),
            local_port,
            sent_local_port: false,
            recv_peer_port: None,
        })
    };
    ////////////////////////////////////////////

    // 1. Start to construct EndpointManager
    let mut poll = Poll::new().map_err(drop)?;
    let mut events = Events::with_capacity(64);
    let mut undrained_endpoints = IndexSet::<usize>::default();

    // 2. create a registered (TcpListener/Endpoint) for passive / active respectively
    let mut todos = endpoint_setups
        .iter()
        .enumerate()
        .map(|(index, (local_port, endpoint_setup))| {
            init_todo(Token(index), *local_port, endpoint_setup, &mut poll)
        })
        .collect::<Result<Vec<Todo>, _>>()?;

    // 3. Using poll to drive progress:
    //    - accept an incoming connection for each TcpListener (turning them into endpoints too)
    //    - for each endpoint, send the local PortId
    //    - for each endpoint, recv the peer's PortId, and
    let mut setup_incomplete: HashSet<usize> = (0..todos.len()).collect();
    while !setup_incomplete.is_empty() {
        let remaining = deadline.checked_duration_since(Instant::now()).ok_or(())?;
        poll.poll(&mut events, Some(remaining)).map_err(drop)?;
        for event in events.iter() {
            let token = event.token();
            let Token(index) = token;
            let todo: &mut Todo = &mut todos[index];
            if let TodoEndpoint::Listener(listener) = &mut todo.todo_endpoint {
                let (mut stream, peer_addr) = listener.accept().map_err(drop)?;
                poll.registry().deregister(listener).unwrap();
                poll.registry().register(&mut stream, token, BOTH).unwrap();
                log!(logger, "Endpoint({}) accepted a connection from {:?}", index, peer_addr);
                let endpoint = Endpoint { stream, inbox: vec![] };
                todo.todo_endpoint = TodoEndpoint::Endpoint(endpoint);
            }
            match todo {
                Todo {
                    todo_endpoint: TodoEndpoint::Endpoint(endpoint),
                    local_port,
                    endpoint_setup,
                    sent_local_port,
                    recv_peer_port,
                } => {
                    if !setup_incomplete.contains(&index) {
                        continue;
                    }
                    if event.is_writable() && !*sent_local_port {
                        let msg =
                            MyPortInfo { polarity: endpoint_setup.polarity, port: *local_port };
                        endpoint.send(&msg)?;
                        log!(logger, "endpoint[{}] sent peer info {:?}", index, &msg);
                        *sent_local_port = true;
                    }
                    if event.is_readable() && recv_peer_port.is_none() {
                        undrained_endpoints.insert(index);
                        if let Some(peer_port_info) =
                            endpoint.try_recv::<MyPortInfo>().map_err(drop)?
                        {
                            log!(logger, "endpoint[{}] got peer info {:?}", index, peer_port_info);
                            assert!(peer_port_info.polarity != endpoint_setup.polarity);
                            *recv_peer_port = Some(peer_port_info.port);
                            let lci = LogicalChannelInfo {
                                local_port: *local_port,
                                peer_port: peer_port_info.port,
                                local_polarity: endpoint_setup.polarity,
                                endpoint_index: index,
                            };
                            logical_channel_callback(lci);
                        }
                    }
                    if *sent_local_port && recv_peer_port.is_some() {
                        setup_incomplete.remove(&index);
                        log!(logger, "endpoint[{}] is finished!", index);
                    }
                }
                Todo { todo_endpoint: TodoEndpoint::Listener(_), .. } => unreachable!(),
            }
        }
        events.clear();
    }
    let endpoint_exts = todos
        .into_iter()
        .map(|Todo { todo_endpoint, recv_peer_port, .. }| EndpointExt {
            endpoint: match todo_endpoint {
                TodoEndpoint::Endpoint(endpoint) => endpoint,
                TodoEndpoint::Listener(..) => unreachable!(),
            },
            inp_for_emerging_msgs: recv_peer_port.unwrap(),
        })
        .collect();
    Ok(EndpointManager {
        poll,
        events,
        undrained_endpoints,
        delayed_messages: Default::default(),
        undelayed_messages: Default::default(),
        endpoint_exts,
    })
}

fn init_neighborhood(
    controller_id: ControllerId,
    logger: &mut dyn Logger,
    em: &mut EndpointManager,
    deadline: Instant,
) -> Result<Neighborhood, ()> {
    ////////////////////////////////////////////
    use Msg::SetupMsg as S;
    use SetupMsg::*;
    ////////////////////////////////////////////

    log!(logger, "beginning neighborhood construction");
    // 1. broadcast my ID as the first echo. await reply from all neighbors
    let echo = S(LeaderEcho { maybe_leader: controller_id });
    let mut awaiting = HashSet::with_capacity(em.endpoint_exts.len());
    for (index, ee) in em.endpoint_exts.iter_mut().enumerate() {
        log!(logger, "{:?}'s initial echo to {:?}, {:?}", controller_id, index, &echo);
        ee.endpoint.send(&echo)?;
        awaiting.insert(index);
    }

    // 2. Receive incoming replies. whenever a higher-id echo arrives,
    //    adopt it as leader, sender as parent, and reset the await set.
    let mut parent: Option<usize> = None;
    let mut my_leader = controller_id;
    em.undelay_all();
    'echo_loop: while !awaiting.is_empty() || parent.is_some() {
        let (index, msg) = em.try_recv_any(deadline).map_err(drop)?;
        log!(logger, "GOT from index {:?} msg {:?}", &index, &msg);
        match msg {
            S(LeaderAnnounce { leader }) => {
                // someone else completed the echo and became leader first!
                // the sender is my parent
                parent = Some(index);
                my_leader = leader;
                awaiting.clear();
                break 'echo_loop;
            }
            S(LeaderEcho { maybe_leader }) => {
                use Ordering::*;
                match maybe_leader.cmp(&my_leader) {
                    Less => { /* ignore */ }
                    Equal => {
                        awaiting.remove(&index);
                        if awaiting.is_empty() {
                            if let Some(p) = parent {
                                // return the echo to my parent
                                em.send_to(p, &S(LeaderEcho { maybe_leader }))?;
                            } else {
                                // DECIDE!
                                break 'echo_loop;
                            }
                        }
                    }
                    Greater => {
                        // join new echo
                        log!(logger, "Setting leader to index {:?}", index);
                        parent = Some(index);
                        my_leader = maybe_leader;
                        let echo = S(LeaderEcho { maybe_leader: my_leader });
                        awaiting.clear();
                        if em.endpoint_exts.len() == 1 {
                            // immediately reply to parent
                            log!(logger, "replying echo to parent {:?} immediately", index);
                            em.send_to(index, &echo)?;
                        } else {
                            for (index2, ee) in em.endpoint_exts.iter_mut().enumerate() {
                                if index2 == index {
                                    continue;
                                }
                                log!(logger, "repeating echo {:?} to {:?}", &echo, index2);
                                ee.endpoint.send(&echo)?;
                                awaiting.insert(index2);
                            }
                        }
                    }
                }
            }
            inappropriate_msg => em.delayed_messages.push((index, inappropriate_msg)),
        }
    }
    match parent {
        None => assert_eq!(
            my_leader, controller_id,
            "I've got no parent, but I consider {:?} the leader?",
            my_leader
        ),
        Some(parent) => assert_ne!(
            my_leader, controller_id,
            "I have {:?} as parent, but I consider myself ({:?}) the leader?",
            parent, controller_id
        ),
    }
    log!(logger, "DONE WITH ECHO! Leader has cid={:?}", my_leader);

    // 3. broadcast leader announcement (except to parent: confirm they are your parent)
    //    in this loop, every node sends 1 message to each neighbor
    //    await 1 message from all non-parents.
    let msg_for_non_parents = S(LeaderAnnounce { leader: my_leader });
    for (index, ee) in em.endpoint_exts.iter_mut().enumerate() {
        let msg = if Some(index) == parent {
            &S(YouAreMyParent)
        } else {
            awaiting.insert(index);
            &msg_for_non_parents
        };
        log!(logger, "ANNOUNCING to {:?} {:?}", index, msg);
        ee.endpoint.send(msg)?;
    }
    let mut children = Vec::default();
    em.undelay_all();
    while !awaiting.is_empty() {
        let (index, msg) = em.try_recv_any(deadline).map_err(drop)?;
        match msg {
            S(YouAreMyParent) => {
                assert!(awaiting.remove(&index));
                children.push(index);
            }
            S(SetupMsg::LeaderAnnounce { leader }) => {
                assert!(awaiting.remove(&index));
                assert!(leader == my_leader);
                assert!(Some(index) != parent);
                // they wouldn't send me this if they considered me their parent
            }
            inappropriate_msg => em.delayed_messages.push((index, inappropriate_msg)),
        }
    }
    children.sort();
    children.dedup();
    Ok(Neighborhood { parent, children })
}
