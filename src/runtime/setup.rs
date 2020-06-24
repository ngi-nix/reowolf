use crate::common::*;
use crate::runtime::*;
use std::io::ErrorKind::WouldBlock;

impl Connector {
    pub fn new_simple(
        proto_description: Arc<ProtocolDescription>,
        connector_id: ConnectorId,
    ) -> Self {
        let logger = Box::new(DummyLogger);
        // let logger = Box::new(DummyLogger);
        let surplus_sockets = 2;
        Self::new(logger, proto_description, connector_id, surplus_sockets)
    }
    pub fn new(
        mut logger: Box<dyn Logger>,
        proto_description: Arc<ProtocolDescription>,
        connector_id: ConnectorId,
        surplus_sockets: u16,
    ) -> Self {
        log!(&mut *logger, "Created with connector_id {:?}", connector_id);
        Self {
            proto_description,
            proto_components: Default::default(),
            logger,
            id_manager: IdManager::new(connector_id),
            native_ports: Default::default(),
            port_info: Default::default(),
            phased: ConnectorPhased::Setup { endpoint_setups: Default::default(), surplus_sockets },
        }
    }
    pub fn new_net_port(
        &mut self,
        polarity: Polarity,
        sock_addr: SocketAddr,
        endpoint_polarity: EndpointPolarity,
    ) -> Result<PortId, ()> {
        match &mut self.phased {
            ConnectorPhased::Setup { endpoint_setups, .. } => {
                let endpoint_setup = EndpointSetup { sock_addr, endpoint_polarity };
                let p = self.id_manager.new_port_id();
                self.native_ports.insert(p);
                // {polarity, route} known. {peer} unknown.
                self.port_info.polarities.insert(p, polarity);
                self.port_info.routes.insert(p, Route::LocalComponent(LocalComponentId::Native));
                log!(
                    self.logger,
                    "Added net port {:?} with polarity {:?} and endpoint setup {:?} ",
                    p,
                    polarity,
                    &endpoint_setup
                );
                endpoint_setups.push((p, endpoint_setup));
                Ok(p)
            }
            ConnectorPhased::Communication { .. } => Err(()),
        }
    }
    pub fn connect(&mut self, timeout: Option<Duration>) -> Result<(), ConnectError> {
        use ConnectError::*;
        match &mut self.phased {
            ConnectorPhased::Communication { .. } => {
                log!(self.logger, "Call to connecting in connected state");
                Err(AlreadyConnected)
            }
            ConnectorPhased::Setup { endpoint_setups, .. } => {
                log!(self.logger, "~~~ CONNECT called timeout {:?}", timeout);
                let deadline = timeout.map(|to| Instant::now() + to);
                // connect all endpoints in parallel; send and receive peer ids through ports
                let mut endpoint_manager = new_endpoint_manager(
                    &mut *self.logger,
                    endpoint_setups,
                    &mut self.port_info,
                    deadline,
                )?;
                log!(
                    self.logger,
                    "Successfully connected {} endpoints",
                    endpoint_manager.endpoint_exts.len()
                );
                // leader election and tree construction
                let neighborhood = init_neighborhood(
                    self.id_manager.connector_id,
                    &mut *self.logger,
                    &mut endpoint_manager,
                    deadline,
                )?;
                log!(self.logger, "Successfully created neighborhood {:?}", &neighborhood);
                log!(self.logger, "connect() finished. setup phase complete");
                // TODO session optimization goes here
                self.phased = ConnectorPhased::Communication {
                    round_index: 0,
                    endpoint_manager,
                    neighborhood,
                    mem_inbox: Default::default(),
                    native_batches: vec![Default::default()],
                    round_result: Ok(None),
                };
                Ok(())
            }
        }
    }
}

fn new_endpoint_manager(
    logger: &mut dyn Logger,
    endpoint_setups: &[(PortId, EndpointSetup)],
    port_info: &mut PortInfo,
    deadline: Option<Instant>,
) -> Result<EndpointManager, ConnectError> {
    ////////////////////////////////////////////
    use ConnectError::*;
    const BOTH: Interest = Interest::READABLE.add(Interest::WRITABLE);
    struct Todo {
        todo_endpoint: TodoEndpoint,
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
    ) -> Result<Todo, ConnectError> {
        let todo_endpoint = if let EndpointPolarity::Active = endpoint_setup.endpoint_polarity {
            let mut stream = TcpStream::connect(endpoint_setup.sock_addr)
                .expect("mio::TcpStream connect should not fail!");
            poll.registry().register(&mut stream, token, BOTH).unwrap();
            TodoEndpoint::Endpoint(Endpoint { stream, inbox: vec![] })
        } else {
            let mut listener = TcpListener::bind(endpoint_setup.sock_addr)
                .map_err(|_| BindFailed(endpoint_setup.sock_addr))?;
            poll.registry().register(&mut listener, token, BOTH).unwrap();
            TodoEndpoint::Listener(listener)
        };
        Ok(Todo { todo_endpoint, local_port, sent_local_port: false, recv_peer_port: None })
    };
    ////////////////////////////////////////////

    // 1. Start to construct EndpointManager
    let mut poll = Poll::new().map_err(|_| PollInitFailed)?;
    let mut events = Events::with_capacity(64);
    let mut polled_undrained = IndexSet::<usize>::default();
    let mut delayed_messages = vec![];

    // 2. create a registered (TcpListener/Endpoint) for passive / active respectively
    let mut todos = endpoint_setups
        .iter()
        .enumerate()
        .map(|(index, (local_port, endpoint_setup))| {
            init_todo(Token(index), *local_port, endpoint_setup, &mut poll)
        })
        .collect::<Result<Vec<Todo>, ConnectError>>()?;

    // 3. Using poll to drive progress:
    //    - accept an incoming connection for each TcpListener (turning them into endpoints too)
    //    - for each endpoint, send the local PortId
    //    - for each endpoint, recv the peer's PortId, and
    let mut setup_incomplete: HashSet<usize> = (0..todos.len()).collect();
    while !setup_incomplete.is_empty() {
        let remaining = if let Some(deadline) = deadline {
            Some(deadline.checked_duration_since(Instant::now()).ok_or(Timeout)?)
        } else {
            None
        };
        poll.poll(&mut events, remaining).map_err(|_| PollFailed)?;
        for event in events.iter() {
            let token = event.token();
            let Token(index) = token;
            let todo: &mut Todo = &mut todos[index];
            if let TodoEndpoint::Listener(listener) = &mut todo.todo_endpoint {
                match listener.accept() {
                    Ok((mut stream, peer_addr)) => {
                        poll.registry().deregister(listener).unwrap();
                        poll.registry().register(&mut stream, token, BOTH).unwrap();
                        log!(
                            logger,
                            "Endpoint[{}] accepted a connection from {:?}",
                            index,
                            peer_addr
                        );
                        let endpoint = Endpoint { stream, inbox: vec![] };
                        todo.todo_endpoint = TodoEndpoint::Endpoint(endpoint);
                    }
                    Err(e) if e.kind() == WouldBlock => {}
                    Err(_) => return Err(AcceptFailed(listener.local_addr().unwrap())),
                }
            }
            match todo {
                Todo {
                    todo_endpoint: TodoEndpoint::Endpoint(endpoint),
                    local_port,
                    sent_local_port,
                    recv_peer_port,
                    ..
                } => {
                    if !setup_incomplete.contains(&index) {
                        continue;
                    }
                    let local_polarity = *port_info.polarities.get(local_port).unwrap();
                    if event.is_writable() && !*sent_local_port {
                        let msg = Msg::SetupMsg(SetupMsg::MyPortInfo(MyPortInfo {
                            polarity: local_polarity,
                            port: *local_port,
                        }));
                        endpoint
                            .send(&msg)
                            .map_err(|e| {
                                EndpointSetupError(endpoint.stream.local_addr().unwrap(), e)
                            })
                            .unwrap();
                        log!(logger, "endpoint[{}] sent msg {:?}", index, &msg);
                        *sent_local_port = true;
                    }
                    if event.is_readable() && recv_peer_port.is_none() {
                        let maybe_msg = endpoint.try_recv(logger).map_err(|e| {
                            EndpointSetupError(endpoint.stream.local_addr().unwrap(), e)
                        })?;
                        if maybe_msg.is_some() && !endpoint.inbox.is_empty() {
                            polled_undrained.insert(index);
                        }
                        match maybe_msg {
                            None => {} // msg deserialization incomplete
                            Some(Msg::SetupMsg(SetupMsg::MyPortInfo(peer_info))) => {
                                log!(logger, "endpoint[{}] got peer info {:?}", index, peer_info);
                                if peer_info.polarity == local_polarity {
                                    return Err(ConnectError::PortPeerPolarityMismatch(
                                        *local_port,
                                    ));
                                }
                                *recv_peer_port = Some(peer_info.port);
                                // 1. finally learned the peer of this port!
                                port_info.peers.insert(*local_port, peer_info.port);
                                // 2. learned the info of this peer port
                                port_info.polarities.insert(peer_info.port, peer_info.polarity);
                                port_info.peers.insert(peer_info.port, *local_port);
                                port_info.routes.insert(peer_info.port, Route::Endpoint { index });
                            }
                            Some(inappropriate_msg) => {
                                log!(
                                    logger,
                                    "delaying msg {:?} during channel setup phase",
                                    inappropriate_msg
                                );
                                delayed_messages.push((index, inappropriate_msg));
                            }
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
        .enumerate()
        .map(|(index, Todo { todo_endpoint, local_port, .. })| EndpointExt {
            endpoint: match todo_endpoint {
                TodoEndpoint::Endpoint(mut endpoint) => {
                    poll.registry()
                        .reregister(&mut endpoint.stream, Token(index), Interest::READABLE)
                        .unwrap();
                    endpoint
                }
                TodoEndpoint::Listener(..) => unreachable!(),
            },
            getter_for_incoming: local_port,
        })
        .collect();
    Ok(EndpointManager {
        poll,
        events,
        polled_undrained,
        undelayed_messages: delayed_messages, // no longer delayed
        delayed_messages: Default::default(),
        endpoint_exts,
    })
}

fn init_neighborhood(
    connector_id: ConnectorId,
    logger: &mut dyn Logger,
    em: &mut EndpointManager,
    deadline: Option<Instant>,
) -> Result<Neighborhood, ConnectError> {
    use {ConnectError::*, Msg::SetupMsg as S, SetupMsg::*};
    ////////////////////////////////
    #[derive(Debug)]
    struct WaveState {
        parent: Option<usize>,
        leader: ConnectorId,
    }
    fn do_wave(
        em: &mut EndpointManager,
        awaiting: &mut HashSet<usize>,
        ws: &WaveState,
    ) -> Result<(), ConnectError> {
        awaiting.clear();
        let msg = S(LeaderWave { wave_leader: ws.leader });
        for index in em.index_iter() {
            if Some(index) != ws.parent {
                em.send_to_setup(index, &msg)?;
                awaiting.insert(index);
            }
        }
        Ok(())
    }
    ///////////////////////
    /*
    Conceptually, we have two distinct disstributed algorithms back-to-back
    1. Leader election using echo algorithm with extinction.
        - Each connector initiates a wave tagged with their ID
        - Connectors participate in waves of GREATER ID, abandoning previous waves
        - Only the wave of the connector with GREATEST ID completes, whereupon they are the leader
    2. Tree construction
        - The leader broadcasts their leadership with msg A
        - Upon receiving their first announcement, connectors reply B, and send A to all peers
        - A controller exits once they have received A or B from each neighbor

    The actual implementation is muddier, because non-leaders aren't aware of termiantion of algorithm 1,
    so they rely on receipt of the leader's announcement to realize that algorithm 2 has begun.

    NOTE the distinction between PARENT and LEADER
    */
    log!(logger, "beginning neighborhood construction");
    if em.num_endpoints() == 0 {
        log!(logger, "Edge case of no neighbors! No parent an no children!");
        return Ok(Neighborhood { parent: None, children: VecSet::new(vec![]) });
    }
    log!(logger, "Have {} endpoints. Must participate in distributed alg.", em.num_endpoints());
    let mut awaiting = HashSet::with_capacity(em.num_endpoints());
    // 1+ neighbors. Leader can only be learned by receiving messages
    // loop ends when I know my sink tree parent (implies leader was elected)
    let election_result: WaveState = {
        // initially: No parent, I'm the best leader.
        let mut best_wave = WaveState { parent: None, leader: connector_id };
        // start a wave for this initial state
        do_wave(em, &mut awaiting, &best_wave)?;
        // with 1+ neighbors, progress is only made in response to incoming messages
        em.undelay_all();
        'election: loop {
            log!(logger, "Election loop. awaiting {:?}...", awaiting.iter());
            let (recv_index, msg) = em.try_recv_any_setup(logger, deadline)?;
            log!(logger, "Received from index {:?} msg {:?}", &recv_index, &msg);
            match msg {
                S(LeaderAnnounce { tree_leader }) => {
                    let election_result =
                        WaveState { leader: tree_leader, parent: Some(recv_index) };
                    log!(logger, "Election lost! Result {:?}", &election_result);
                    assert!(election_result.leader >= best_wave.leader);
                    assert_ne!(election_result.leader, connector_id);
                    break 'election election_result;
                }
                S(LeaderWave { wave_leader }) => {
                    use Ordering as O;
                    match wave_leader.cmp(&best_wave.leader) {
                        O::Less => log!(
                            logger,
                            "Ignoring wave with Id {:?}<{:?}",
                            wave_leader,
                            best_wave.leader
                        ),
                        O::Greater => {
                            log!(
                                logger,
                                "Joining wave with Id {:?}>{:?}",
                                wave_leader,
                                best_wave.leader
                            );
                            best_wave = WaveState { leader: wave_leader, parent: Some(recv_index) };
                            log!(logger, "New wave state {:?}", &best_wave);
                            do_wave(em, &mut awaiting, &best_wave)?;
                            if awaiting.is_empty() {
                                log!(logger, "Special case! Only neighbor is parent. Replying to {:?} msg {:?}", recv_index, &msg);
                                em.send_to_setup(recv_index, &msg)?;
                            }
                        }
                        O::Equal => {
                            assert!(awaiting.remove(&recv_index));
                            log!(
                                logger,
                                "Wave reply from index {:?} for leader {:?}. Now awaiting {} replies",
                                recv_index,
                                best_wave.leader,
                                awaiting.len()
                            );
                            if awaiting.is_empty() {
                                if let Some(parent) = best_wave.parent {
                                    log!(
                                        logger,
                                        "Sub-wave done! replying to parent {:?} msg {:?}",
                                        parent,
                                        &msg
                                    );
                                    em.send_to_setup(parent, &msg)?;
                                } else {
                                    let election_result: WaveState = best_wave;
                                    log!(logger, "Election won! Result {:?}", &election_result);
                                    break 'election election_result;
                                }
                            }
                        }
                    }
                }
                S(YouAreMyParent) | S(MyPortInfo(_)) => unreachable!(),
                comm_msg @ Msg::CommMsg { .. } => {
                    log!(logger, "delaying msg {:?} during election algorithm", comm_msg);
                    em.delayed_messages.push((recv_index, comm_msg));
                }
            }
        }
    };

    // starting algorithm 2. Send a message to every neighbor
    log!(logger, "Starting tree construction. Step 1: send one msg per neighbor");
    awaiting.clear();
    for index in em.index_iter() {
        if Some(index) == election_result.parent {
            em.send_to_setup(index, &S(YouAreMyParent))?;
        } else {
            awaiting.insert(index);
            em.send_to_setup(index, &S(LeaderAnnounce { tree_leader: election_result.leader }))?;
        }
    }
    let mut children = vec![];
    em.undelay_all();
    while !awaiting.is_empty() {
        log!(logger, "Tree construction_loop loop. awaiting {:?}...", awaiting.iter());
        let (recv_index, msg) = em.try_recv_any_setup(logger, deadline)?;
        log!(logger, "Received from index {:?} msg {:?}", &recv_index, &msg);
        match msg {
            S(LeaderWave { .. }) => { /* old message */ }
            S(LeaderAnnounce { .. }) => {
                // not a child
                log!(
                    logger,
                    "Got reply from non-child index {:?}. Children: {:?}",
                    recv_index,
                    children.iter()
                );
                if !awaiting.remove(&recv_index) {
                    return Err(SetupAlgMisbehavior);
                }
            }
            S(YouAreMyParent) => {
                if !awaiting.remove(&recv_index) {
                    log!(
                        logger,
                        "Got reply from child index {:?}. Children before... {:?}",
                        recv_index,
                        children.iter()
                    );
                    return Err(SetupAlgMisbehavior);
                }
                children.push(recv_index);
            }
            S(MyPortInfo(_)) => unreachable!(),
            comm_msg @ Msg::CommMsg { .. } => {
                log!(logger, "delaying msg {:?} during election algorithm", comm_msg);
                em.delayed_messages.push((recv_index, comm_msg));
            }
        }
    }
    children.shrink_to_fit();
    let neighborhood =
        Neighborhood { parent: election_result.parent, children: VecSet::new(children) };
    log!(logger, "Neighborhood constructed {:?}", &neighborhood);
    Ok(neighborhood)
}
