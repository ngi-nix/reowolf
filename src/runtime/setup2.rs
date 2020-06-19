use crate::common::*;
use crate::runtime::*;

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
                let (mut endpoint_exts, mut endpoint_poller) = init_endpoints(
                    &mut *self.logger,
                    endpoint_setups,
                    &mut self.inp_to_route,
                    deadline,
                )?;
                log!(self.logger, "Successfully connected {} endpoints", endpoint_exts.len());
                // leader election and tree construction
                let neighborhood = init_neighborhood(
                    self.id_manager.controller_id,
                    &mut *self.logger,
                    &mut endpoint_exts,
                    &mut endpoint_poller,
                    deadline,
                )?;
                log!(self.logger, "Successfully created neighborhood {:?}", &neighborhood);
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
    logger: &mut dyn Logger,
    endpoint_setups: &[(PortId, EndpointSetup)],
    inp_to_route: &mut HashMap<PortId, InpRoute>,
    deadline: Instant,
) -> Result<(Vec<EndpointExt>, EndpointPoller), ()> {
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
    fn init(
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
    ////////////////////////

    let mut ep = EndpointPoller {
        poll: Poll::new().map_err(drop)?,
        events: Events::with_capacity(64),
        undrained_endpoints: Default::default(),
        delayed_messages: Default::default(),
        undelayed_messages: Default::default(),
    };

    let mut todos = endpoint_setups
        .iter()
        .enumerate()
        .map(|(index, (local_port, endpoint_setup))| {
            init(Token(index), *local_port, endpoint_setup, &mut ep.poll)
        })
        .collect::<Result<Vec<Todo>, _>>()?;

    let mut unfinished: HashSet<usize> = (0..todos.len()).collect();
    while !unfinished.is_empty() {
        let remaining = deadline.checked_duration_since(Instant::now()).ok_or(())?;
        ep.poll.poll(&mut ep.events, Some(remaining)).map_err(drop)?;
        for event in ep.events.iter() {
            let token = event.token();
            let Token(index) = token;
            let todo: &mut Todo = &mut todos[index];
            if let TodoEndpoint::Listener(listener) = &mut todo.todo_endpoint {
                let (mut stream, peer_addr) = listener.accept().map_err(drop)?;
                ep.poll.registry().deregister(listener).unwrap();
                ep.poll.registry().register(&mut stream, token, BOTH).unwrap();
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
                    if !unfinished.contains(&index) {
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
                        ep.undrained_endpoints.insert(index);
                        if let Some(peer_port_info) =
                            endpoint.try_recv::<MyPortInfo>().map_err(drop)?
                        {
                            log!(logger, "endpoint[{}] got peer info {:?}", index, peer_port_info);
                            assert!(peer_port_info.polarity != endpoint_setup.polarity);
                            if let Putter = endpoint_setup.polarity {
                                inp_to_route.insert(*local_port, InpRoute::Endpoint { index });
                            }
                            *recv_peer_port = Some(peer_port_info.port);
                        }
                    }
                    if *sent_local_port && recv_peer_port.is_some() {
                        unfinished.remove(&index);
                        log!(logger, "endpoint[{}] is finished!", index);
                    }
                }
                Todo { todo_endpoint: TodoEndpoint::Listener(_), .. } => unreachable!(),
            }
        }
        ep.events.clear();
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
    Ok((endpoint_exts, ep))
}

fn init_neighborhood(
    controller_id: ControllerId,
    logger: &mut dyn Logger,
    endpoint_exts: &mut [EndpointExt],
    ep: &mut EndpointPoller,
    deadline: Instant,
) -> Result<Neighborhood, ()> {
    log!(logger, "beginning neighborhood construction");
    use Msg::SetupMsg as S;
    use SetupMsg::*;

    // 1. broadcast my ID as the first echo. await reply from all neighbors
    let echo = S(LeaderEcho { maybe_leader: controller_id });
    let mut awaiting = HashSet::with_capacity(endpoint_exts.len());
    for (index, ee) in endpoint_exts.iter_mut().enumerate() {
        log!(logger, "{:?}'s initial echo to {:?}, {:?}", controller_id, index, &echo);
        ee.endpoint.send(&echo)?;
        awaiting.insert(index);
    }

    // 2. Receive incoming replies. whenever a higher-id echo arrives,
    //    adopt it as leader, sender as parent, and reset the await set.
    let mut parent: Option<usize> = None;
    let mut my_leader = controller_id;
    ep.undelay_all();
    'echo_loop: while !awaiting.is_empty() || parent.is_some() {
        let (index, msg) = ep.try_recv_any(endpoint_exts, deadline).map_err(drop)?;
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
                                endpoint_exts[p].endpoint.send(&S(LeaderEcho { maybe_leader }))?;
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
                        if endpoint_exts.len() == 1 {
                            // immediately reply to parent
                            log!(logger, "replying echo to parent {:?} immediately", index);
                            endpoint_exts[index].endpoint.send(&echo)?;
                        } else {
                            for (index2, ee) in endpoint_exts.iter_mut().enumerate() {
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
            inappropriate_msg => ep.delayed_messages.push((index, inappropriate_msg)),
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
    // await 1 message from all non-parents
    let msg_for_non_parents = S(LeaderAnnounce { leader: my_leader });
    for (index, ee) in endpoint_exts.iter_mut().enumerate() {
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
    ep.undelay_all();
    while !awaiting.is_empty() {
        let (index, msg) = ep.try_recv_any(endpoint_exts, deadline).map_err(drop)?;
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
            inappropriate_msg => ep.delayed_messages.push((index, inappropriate_msg)),
        }
    }
    children.sort();
    children.dedup();
    Ok(Neighborhood { parent, children })
}
