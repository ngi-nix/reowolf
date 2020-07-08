use crate::common::*;
use crate::runtime::*;

impl Connector {
    pub fn new(
        mut logger: Box<dyn Logger>,
        proto_description: Arc<ProtocolDescription>,
        connector_id: ConnectorId,
        surplus_sockets: u16,
    ) -> Self {
        log!(&mut *logger, "Created with connector_id {:?}", connector_id);
        Self {
            unphased: ConnectorUnphased {
                proto_description,
                proto_components: Default::default(),
                logger,
                id_manager: IdManager::new(connector_id),
                native_ports: Default::default(),
                port_info: Default::default(),
            },
            phased: ConnectorPhased::Setup(Box::new(ConnectorSetup {
                net_endpoint_setups: Default::default(),
                udp_endpoint_setups: Default::default(),
                surplus_sockets,
            })),
        }
    }
    pub fn new_udp_port(
        &mut self,
        polarity: Polarity,
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
    ) -> Result<PortId, WrongStateError> {
        let Self { unphased: cu, phased } = self;
        match phased {
            ConnectorPhased::Communication(..) => Err(WrongStateError),
            ConnectorPhased::Setup(setup) => {
                let udp_endpoint_setup = UdpEndpointSetup { local_addr, peer_addr };
                let udp_index = setup.udp_endpoint_setups.len();
                let [port_nat, port_udp] =
                    [cu.id_manager.new_port_id(), cu.id_manager.new_port_id()];
                cu.native_ports.insert(port_nat);
                cu.port_info.peers.insert(port_nat, port_udp);
                cu.port_info.peers.insert(port_udp, port_nat);
                cu.port_info.routes.insert(port_nat, Route::LocalComponent(ComponentId::Native));
                cu.port_info.routes.insert(port_udp, Route::UdpEndpoint { index: udp_index });
                cu.port_info.polarities.insert(port_nat, polarity);
                cu.port_info.polarities.insert(port_udp, !polarity);
                setup.udp_endpoint_setups.push((port_nat, udp_endpoint_setup));
                Ok(port_nat)
            }
        }
    }
    pub fn new_net_port(
        &mut self,
        polarity: Polarity,
        sock_addr: SocketAddr,
        endpoint_polarity: EndpointPolarity,
    ) -> Result<PortId, WrongStateError> {
        let Self { unphased: cu, phased } = self;
        match phased {
            ConnectorPhased::Communication(..) => Err(WrongStateError),
            ConnectorPhased::Setup(setup) => {
                let endpoint_setup = NetEndpointSetup { sock_addr, endpoint_polarity };
                let p = cu.id_manager.new_port_id();
                cu.native_ports.insert(p);
                // {polarity, route} known. {peer} unknown.
                cu.port_info.polarities.insert(p, polarity);
                cu.port_info.routes.insert(p, Route::LocalComponent(ComponentId::Native));
                log!(
                    cu.logger,
                    "Added net port {:?} with polarity {:?} and endpoint setup {:?} ",
                    p,
                    polarity,
                    &endpoint_setup
                );
                setup.net_endpoint_setups.push((p, endpoint_setup));
                Ok(p)
            }
        }
    }
    pub fn connect(&mut self, timeout: Option<Duration>) -> Result<(), ConnectError> {
        use ConnectError as Ce;
        let Self { unphased: cu, phased } = self;
        match &phased {
            ConnectorPhased::Communication { .. } => {
                log!(cu.logger, "Call to connecting in connected state");
                Err(Ce::AlreadyConnected)
            }
            ConnectorPhased::Setup(setup) => {
                log!(cu.logger, "~~~ CONNECT called timeout {:?}", timeout);
                let deadline = timeout.map(|to| Instant::now() + to);
                // connect all endpoints in parallel; send and receive peer ids through ports
                let mut endpoint_manager = new_endpoint_manager(
                    &mut *cu.logger,
                    &setup.net_endpoint_setups,
                    &mut cu.port_info,
                    &deadline,
                )?;
                log!(
                    cu.logger,
                    "Successfully connected {} endpoints",
                    endpoint_manager.net_endpoint_store.endpoint_exts.len()
                );
                // leader election and tree construction
                let neighborhood = init_neighborhood(
                    cu.id_manager.connector_id,
                    &mut *cu.logger,
                    &mut endpoint_manager,
                    &deadline,
                )?;
                log!(cu.logger, "Successfully created neighborhood {:?}", &neighborhood);
                let mut comm = ConnectorCommunication {
                    round_index: 0,
                    endpoint_manager,
                    neighborhood,
                    native_batches: vec![Default::default()],
                    round_result: Ok(None),
                };
                if cfg!(feature = "session_optimization") {
                    session_optimize(cu, &mut comm, &deadline)?;
                }
                log!(cu.logger, "connect() finished. setup phase complete");
                self.phased = ConnectorPhased::Communication(Box::new(comm));
                Ok(())
            }
        }
    }
}
fn new_endpoint_manager(
    logger: &mut dyn Logger,
    endpoint_setups: &[(PortId, NetEndpointSetup)],
    port_info: &mut PortInfo,
    deadline: &Option<Instant>,
) -> Result<EndpointManager, ConnectError> {
    ////////////////////////////////////////////
    use std::sync::atomic::AtomicBool;
    use ConnectError as Ce;
    const BOTH: Interest = Interest::READABLE.add(Interest::WRITABLE);
    struct Todo {
        todo_endpoint: TodoEndpoint,
        endpoint_setup: NetEndpointSetup,
        local_port: PortId,
        sent_local_port: bool,          // true <-> I've sent my local port
        recv_peer_port: Option<PortId>, // Some(..) <-> I've received my peer's port
    }
    enum TodoEndpoint {
        Accepting(TcpListener),
        NetEndpoint(NetEndpoint),
    }
    fn init_todo(
        token: Token,
        local_port: PortId,
        endpoint_setup: &NetEndpointSetup,
        poll: &mut Poll,
    ) -> Result<Todo, ConnectError> {
        let todo_endpoint = if let EndpointPolarity::Active = endpoint_setup.endpoint_polarity {
            let mut stream = TcpStream::connect(endpoint_setup.sock_addr)
                .expect("mio::TcpStream connect should not fail!");
            poll.registry().register(&mut stream, token, BOTH).unwrap();
            TodoEndpoint::NetEndpoint(NetEndpoint { stream, inbox: vec![] })
        } else {
            let mut listener = TcpListener::bind(endpoint_setup.sock_addr)
                .map_err(|_| Ce::BindFailed(endpoint_setup.sock_addr))?;
            poll.registry().register(&mut listener, token, BOTH).unwrap();
            TodoEndpoint::Accepting(listener)
        };
        Ok(Todo {
            todo_endpoint,
            local_port,
            sent_local_port: false,
            recv_peer_port: None,
            endpoint_setup: endpoint_setup.clone(),
        })
    }
    ////////////////////////////////////////////

    // 1. Start to construct EndpointManager
    const WAKER_PERIOD: Duration = Duration::from_millis(300);
    struct WakerState {
        continue_signal: AtomicBool,
        waker: mio::Waker,
    }

    let mut waker_state: Option<Arc<WakerState>> = None;
    let mut poll = Poll::new().map_err(|_| Ce::PollInitFailed)?;
    let mut events = Events::with_capacity(endpoint_setups.len() * 2 + 4);
    let mut net_polled_undrained = VecSet::default();
    let udp_polled_undrained = VecSet::default();
    let mut delayed_messages = vec![];

    // 2. create a registered (TcpListener/Endpoint) for passive / active respectively
    let mut todos = endpoint_setups
        .iter()
        .enumerate()
        .map(|(index, (local_port, endpoint_setup))| {
            init_todo(
                TokenTarget::NetEndpoint { index }.into(),
                *local_port,
                endpoint_setup,
                &mut poll,
            )
        })
        .collect::<Result<Vec<Todo>, ConnectError>>()?;

    // 3. Using poll to drive progress:
    //    - accept an incoming connection for each TcpListener (turning them into endpoints too)
    //    - for each endpoint, send the local PortId
    //    - for each endpoint, recv the peer's PortId, and

    // all in connect_failed are NOT registered with Poll
    let mut connect_failed: HashSet<usize> = Default::default();

    let mut setup_incomplete: HashSet<usize> = (0..todos.len()).collect();
    while !setup_incomplete.is_empty() {
        let remaining = if let Some(deadline) = deadline {
            Some(deadline.checked_duration_since(Instant::now()).ok_or(Ce::Timeout)?)
        } else {
            None
        };
        poll.poll(&mut events, remaining).map_err(|_| Ce::PollFailed)?;
        for event in events.iter() {
            let token = event.token();
            let token_target = TokenTarget::from(token);
            match token_target {
                TokenTarget::Waker => {
                    log!(
                        logger,
                        "Notification from waker. connect_failed is {:?}",
                        connect_failed.iter()
                    );
                    assert!(waker_state.is_some());
                    for net_endpoint_index in connect_failed.drain() {
                        let todo: &mut Todo = &mut todos[net_endpoint_index];
                        log!(
                            logger,
                            "Restarting connection with endpoint {:?} {:?}",
                            net_endpoint_index,
                            todo.endpoint_setup.sock_addr
                        );
                        match &mut todo.todo_endpoint {
                            TodoEndpoint::NetEndpoint(endpoint) => {
                                let mut new_stream =
                                    TcpStream::connect(todo.endpoint_setup.sock_addr)
                                        .expect("mio::TcpStream connect should not fail!");
                                std::mem::swap(&mut endpoint.stream, &mut new_stream);
                                let token =
                                    TokenTarget::NetEndpoint { index: net_endpoint_index }.into();
                                poll.registry()
                                    .register(&mut endpoint.stream, token, BOTH)
                                    .unwrap();
                            }
                            _ => unreachable!(),
                        }
                    }
                }
                TokenTarget::UdpEndpoint { index: _ } => unreachable!(),
                TokenTarget::NetEndpoint { index } => {
                    let todo: &mut Todo = &mut todos[index];
                    if let TodoEndpoint::Accepting(listener) = &mut todo.todo_endpoint {
                        // FIRST try complete this connection
                        match listener.accept() {
                            Err(e) if would_block(&e) => {
                                log!(logger, "Spurious wakeup on listener {:?}", index)
                            }
                            Err(_) => {
                                log!(logger, "accept() failure on index {}", index);
                                return Err(Ce::AcceptFailed(listener.local_addr().unwrap()));
                            }
                            Ok((mut stream, peer_addr)) => {
                                // success!
                                poll.registry().deregister(listener).unwrap();
                                // reusing original token as-is
                                poll.registry().register(&mut stream, token, BOTH).unwrap();
                                log!(
                                    logger,
                                    "Endpoint[{}] accepted a connection from {:?}",
                                    index,
                                    peer_addr
                                );
                                let net_endpoint = NetEndpoint { stream, inbox: vec![] };
                                todo.todo_endpoint = TodoEndpoint::NetEndpoint(net_endpoint);
                            }
                        }
                    }
                    if let TodoEndpoint::NetEndpoint(net_endpoint) = &mut todo.todo_endpoint {
                        if event.is_error() {
                            if todo.endpoint_setup.endpoint_polarity == EndpointPolarity::Passive {
                                // right now you cannot retry an acceptor. return failure
                                return Err(Ce::AcceptFailed(
                                    net_endpoint.stream.local_addr().unwrap(),
                                ));
                            }
                            // this actively-connecting endpoint failed to connect!
                            if connect_failed.insert(index) {
                                log!(
                                    logger,
                                    "Connection failed for {:?}. List is {:?}",
                                    index,
                                    connect_failed.iter()
                                );
                                poll.registry().deregister(&mut net_endpoint.stream).unwrap();
                            } else {
                                // spurious wakeup.
                                continue;
                            }
                            if waker_state.is_none() {
                                log!(logger, "First connect failure. Starting waker thread");
                                let arc = Arc::new(WakerState {
                                    waker: mio::Waker::new(
                                        poll.registry(),
                                        TokenTarget::Waker.into(),
                                    )
                                    .unwrap(),
                                    continue_signal: true.into(),
                                });
                                let moved_arc = arc.clone();
                                waker_state = Some(arc);
                                std::thread::spawn(move || {
                                    while moved_arc
                                        .continue_signal
                                        .load(std::sync::atomic::Ordering::SeqCst)
                                    {
                                        std::thread::sleep(WAKER_PERIOD);
                                        let _ = moved_arc.waker.wake();
                                    }
                                });
                            }
                            continue;
                        }
                        // event wasn't ERROR
                        if connect_failed.contains(&index) {
                            // spurious wakeup
                            continue;
                        }
                        if !setup_incomplete.contains(&index) {
                            // spurious wakeup
                            continue;
                        }
                        let local_polarity = *port_info.polarities.get(&todo.local_port).unwrap();
                        if event.is_writable() && !todo.sent_local_port {
                            // can write and didn't send setup msg yet? Do so!
                            let msg = Msg::SetupMsg(SetupMsg::MyPortInfo(MyPortInfo {
                                polarity: local_polarity,
                                port: todo.local_port,
                            }));
                            net_endpoint
                                .send(&msg)
                                .map_err(|e| {
                                    Ce::NetEndpointSetupError(
                                        net_endpoint.stream.local_addr().unwrap(),
                                        e,
                                    )
                                })
                                .unwrap();
                            log!(logger, "endpoint[{}] sent msg {:?}", index, &msg);
                            todo.sent_local_port = true;
                        }
                        if event.is_readable() && todo.recv_peer_port.is_none() {
                            // can read and didn't recv setup msg yet? Do so!
                            let maybe_msg = net_endpoint.try_recv(logger).map_err(|e| {
                                Ce::NetEndpointSetupError(
                                    net_endpoint.stream.local_addr().unwrap(),
                                    e,
                                )
                            })?;
                            if maybe_msg.is_some() && !net_endpoint.inbox.is_empty() {
                                net_polled_undrained.insert(index);
                            }
                            match maybe_msg {
                                None => {} // msg deserialization incomplete
                                Some(Msg::SetupMsg(SetupMsg::MyPortInfo(peer_info))) => {
                                    log!(
                                        logger,
                                        "endpoint[{}] got peer info {:?}",
                                        index,
                                        peer_info
                                    );
                                    if peer_info.polarity == local_polarity {
                                        return Err(ConnectError::PortPeerPolarityMismatch(
                                            todo.local_port,
                                        ));
                                    }
                                    todo.recv_peer_port = Some(peer_info.port);
                                    // 1. finally learned the peer of this port!
                                    port_info.peers.insert(todo.local_port, peer_info.port);
                                    // 2. learned the info of this peer port
                                    port_info.polarities.insert(peer_info.port, peer_info.polarity);
                                    port_info.peers.insert(peer_info.port, todo.local_port);
                                    if let Some(route) = port_info.routes.get(&peer_info.port) {
                                        // check just for logging purposes
                                        log!(
                                            logger,
                                            "Special case! Route to peer {:?} already known to be {:?}. Leave untouched",
                                            peer_info.port,
                                            route
                                        );
                                    }
                                    port_info
                                        .routes
                                        .entry(peer_info.port)
                                        .or_insert(Route::NetEndpoint { index });
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
                        // is the setup for this net_endpoint now complete?
                        if todo.sent_local_port && todo.recv_peer_port.is_some() {
                            // yes! connected, sent my info and received peer's info
                            setup_incomplete.remove(&index);
                            log!(logger, "endpoint[{}] is finished!", index);
                        }
                    }
                }
            }
        }
        events.clear();
    }
    log!(logger, "Endpoint setup complete! Cleaning up and building structures");
    if let Some(arc) = waker_state {
        log!(logger, "Sending waker the stop signal");
        arc.continue_signal.store(false, std::sync::atomic::Ordering::SeqCst);
        // TODO leave the waker registered?
    }
    let udp_endpoint_exts = vec![];

    let net_endpoint_exts = todos
        .into_iter()
        .enumerate()
        .map(|(index, Todo { todo_endpoint, local_port, .. })| NetEndpointExt {
            net_endpoint: match todo_endpoint {
                TodoEndpoint::NetEndpoint(mut net_endpoint) => {
                    let token = TokenTarget::NetEndpoint { index }.into();
                    poll.registry()
                        .reregister(&mut net_endpoint.stream, token, Interest::READABLE)
                        .unwrap();
                    net_endpoint
                }
                _ => unreachable!(),
            },
            getter_for_incoming: local_port,
        })
        .collect();
    Ok(EndpointManager {
        poll,
        events,
        undelayed_messages: delayed_messages, // no longer delayed
        delayed_messages: Default::default(),
        net_endpoint_store: EndpointStore {
            endpoint_exts: net_endpoint_exts,
            polled_undrained: net_polled_undrained,
        },
        udp_endpoint_store: EndpointStore {
            endpoint_exts: udp_endpoint_exts,
            polled_undrained: udp_polled_undrained,
        },
        udp_in_buffer: Default::default(),
    })
}

fn init_neighborhood(
    connector_id: ConnectorId,
    logger: &mut dyn Logger,
    em: &mut EndpointManager,
    deadline: &Option<Instant>,
) -> Result<Neighborhood, ConnectError> {
    ////////////////////////////////
    use {ConnectError as Ce, Msg::SetupMsg as S, SetupMsg as Sm};
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
        let msg = S(Sm::LeaderWave { wave_leader: ws.leader });
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
    if em.num_net_endpoints() == 0 {
        log!(logger, "Edge case of no neighbors! No parent an no children!");
        return Ok(Neighborhood { parent: None, children: VecSet::new(vec![]) });
    }
    log!(logger, "Have {} endpoints. Must participate in distributed alg.", em.num_net_endpoints());
    let mut awaiting = HashSet::with_capacity(em.num_net_endpoints());
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
                S(Sm::LeaderAnnounce { tree_leader }) => {
                    let election_result =
                        WaveState { leader: tree_leader, parent: Some(recv_index) };
                    log!(logger, "Election lost! Result {:?}", &election_result);
                    assert!(election_result.leader >= best_wave.leader);
                    assert_ne!(election_result.leader, connector_id);
                    break 'election election_result;
                }
                S(Sm::LeaderWave { wave_leader }) => {
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
                msg @ S(Sm::YouAreMyParent) | msg @ S(Sm::MyPortInfo(_)) => {
                    log!(logger, "Endpont {:?} sent unexpected msg! {:?}", recv_index, &msg);
                    return Err(Ce::SetupAlgMisbehavior);
                }
                msg @ S(Sm::SessionScatter { .. })
                | msg @ S(Sm::SessionGather { .. })
                | msg @ Msg::CommMsg { .. } => {
                    log!(logger, "delaying msg {:?} during election algorithm", msg);
                    em.delayed_messages.push((recv_index, msg));
                }
            }
        }
    };

    // starting algorithm 2. Send a message to every neighbor
    log!(logger, "Starting tree construction. Step 1: send one msg per neighbor");
    awaiting.clear();
    for index in em.index_iter() {
        if Some(index) == election_result.parent {
            em.send_to_setup(index, &S(Sm::YouAreMyParent))?;
        } else {
            awaiting.insert(index);
            em.send_to_setup(
                index,
                &S(Sm::LeaderAnnounce { tree_leader: election_result.leader }),
            )?;
        }
    }
    let mut children = vec![];
    em.undelay_all();
    while !awaiting.is_empty() {
        log!(logger, "Tree construction_loop loop. awaiting {:?}...", awaiting.iter());
        let (recv_index, msg) = em.try_recv_any_setup(logger, deadline)?;
        log!(logger, "Received from index {:?} msg {:?}", &recv_index, &msg);
        match msg {
            S(Sm::LeaderAnnounce { .. }) => {
                // not a child
                log!(
                    logger,
                    "Got reply from non-child index {:?}. Children: {:?}",
                    recv_index,
                    children.iter()
                );
                if !awaiting.remove(&recv_index) {
                    return Err(Ce::SetupAlgMisbehavior);
                }
            }
            S(Sm::YouAreMyParent) => {
                if !awaiting.remove(&recv_index) {
                    log!(
                        logger,
                        "Got reply from child index {:?}. Children before... {:?}",
                        recv_index,
                        children.iter()
                    );
                    return Err(Ce::SetupAlgMisbehavior);
                }
                children.push(recv_index);
            }
            msg @ S(Sm::MyPortInfo(_)) | msg @ S(Sm::LeaderWave { .. }) => {
                log!(logger, "discarding old message {:?} during election", msg);
            }
            msg @ S(Sm::SessionScatter { .. })
            | msg @ S(Sm::SessionGather { .. })
            | msg @ Msg::CommMsg { .. } => {
                log!(logger, "delaying msg {:?} during election", msg);
                em.delayed_messages.push((recv_index, msg));
            }
        }
    }
    children.shrink_to_fit();
    let neighborhood =
        Neighborhood { parent: election_result.parent, children: VecSet::new(children) };
    log!(logger, "Neighborhood constructed {:?}", &neighborhood);
    Ok(neighborhood)
}

fn session_optimize(
    cu: &mut ConnectorUnphased,
    comm: &mut ConnectorCommunication,
    deadline: &Option<Instant>,
) -> Result<(), ConnectError> {
    ////////////////////////////////////////
    use {ConnectError as Ce, Msg::SetupMsg as S, SetupMsg as Sm};
    ////////////////////////////////////////
    log!(cu.logger, "Beginning session optimization");
    // populate session_info_map from a message per child
    let mut unoptimized_map: HashMap<ConnectorId, SessionInfo> = Default::default();
    let mut awaiting: HashSet<usize> = comm.neighborhood.children.iter().copied().collect();
    comm.endpoint_manager.undelay_all();
    while !awaiting.is_empty() {
        log!(
            cu.logger,
            "Session gather loop. awaiting info from children {:?}...",
            awaiting.iter()
        );
        let (recv_index, msg) =
            comm.endpoint_manager.try_recv_any_setup(&mut *cu.logger, deadline)?;
        log!(cu.logger, "Received from index {:?} msg {:?}", &recv_index, &msg);
        match msg {
            S(Sm::SessionGather { unoptimized_map: child_unoptimized_map }) => {
                if !awaiting.remove(&recv_index) {
                    log!(
                        cu.logger,
                        "Wasn't expecting session info from {:?}. Got {:?}",
                        recv_index,
                        &child_unoptimized_map
                    );
                    return Err(Ce::SetupAlgMisbehavior);
                }
                unoptimized_map.extend(child_unoptimized_map.into_iter());
            }
            msg @ S(Sm::YouAreMyParent)
            | msg @ S(Sm::MyPortInfo(..))
            | msg @ S(Sm::LeaderAnnounce { .. })
            | msg @ S(Sm::LeaderWave { .. }) => {
                log!(cu.logger, "discarding old message {:?} during election", msg);
            }
            msg @ S(Sm::SessionScatter { .. }) => {
                log!(
                    cu.logger,
                    "Endpoint {:?} sent unexpected scatter! {:?} I've not contributed yet!",
                    recv_index,
                    &msg
                );
                return Err(Ce::SetupAlgMisbehavior);
            }
            msg @ Msg::CommMsg(..) => {
                log!(cu.logger, "delaying msg {:?} during session optimization", msg);
                comm.endpoint_manager.delayed_messages.push((recv_index, msg));
            }
        }
    }
    log!(
        cu.logger,
        "Gathered all children's maps. ConnectorId set is... {:?}",
        unoptimized_map.keys()
    );
    let my_session_info = SessionInfo {
        port_info: cu.port_info.clone(),
        proto_components: cu.proto_components.clone(),
        serde_proto_description: SerdeProtocolDescription(cu.proto_description.clone()),
        endpoint_incoming_to_getter: comm
            .endpoint_manager
            .net_endpoint_store
            .endpoint_exts
            .iter()
            .map(|ee| ee.getter_for_incoming)
            .collect(),
    };
    unoptimized_map.insert(cu.id_manager.connector_id, my_session_info);
    log!(cu.logger, "Inserting my own info. Unoptimized subtree map is {:?}", &unoptimized_map);

    // acquire the optimized info...
    let optimized_map = if let Some(parent) = comm.neighborhood.parent {
        // ... as a message from my parent
        log!(cu.logger, "Forwarding gathered info to parent {:?}", parent);
        let msg = S(Sm::SessionGather { unoptimized_map });
        comm.endpoint_manager.send_to_setup(parent, &msg)?;
        'scatter_loop: loop {
            log!(
                cu.logger,
                "Session scatter recv loop. awaiting info from children {:?}...",
                awaiting.iter()
            );
            let (recv_index, msg) =
                comm.endpoint_manager.try_recv_any_setup(&mut *cu.logger, deadline)?;
            log!(cu.logger, "Received from index {:?} msg {:?}", &recv_index, &msg);
            match msg {
                S(Sm::SessionScatter { optimized_map }) => {
                    if recv_index != parent {
                        log!(cu.logger, "I expected the scatter from my parent only!");
                        return Err(Ce::SetupAlgMisbehavior);
                    }
                    break 'scatter_loop optimized_map;
                }
                msg @ Msg::CommMsg { .. } => {
                    log!(cu.logger, "delaying msg {:?} during scatter recv", msg);
                    comm.endpoint_manager.delayed_messages.push((recv_index, msg));
                }
                msg @ S(Sm::SessionGather { .. })
                | msg @ S(Sm::YouAreMyParent)
                | msg @ S(Sm::MyPortInfo(..))
                | msg @ S(Sm::LeaderAnnounce { .. })
                | msg @ S(Sm::LeaderWave { .. }) => {
                    log!(cu.logger, "discarding old message {:?} during election", msg);
                }
            }
        }
    } else {
        // by computing it myself
        log!(cu.logger, "I am the leader! I will optimize this session");
        leader_session_map_optimize(&mut *cu.logger, unoptimized_map)?
    };
    log!(
        cu.logger,
        "Optimized info map is {:?}. Sending to children {:?}",
        &optimized_map,
        comm.neighborhood.children.iter()
    );
    log!(cu.logger, "All session info dumped!: {:#?}", &optimized_map);
    let optimized_info =
        optimized_map.get(&cu.id_manager.connector_id).expect("HEY NO INFO FOR ME?").clone();
    let msg = S(Sm::SessionScatter { optimized_map });
    for &child in comm.neighborhood.children.iter() {
        comm.endpoint_manager.send_to_setup(child, &msg)?;
    }
    apply_optimizations(cu, comm, optimized_info)?;
    log!(cu.logger, "Session optimizations applied");
    Ok(())
}
fn leader_session_map_optimize(
    logger: &mut dyn Logger,
    unoptimized_map: HashMap<ConnectorId, SessionInfo>,
) -> Result<HashMap<ConnectorId, SessionInfo>, ConnectError> {
    log!(logger, "Session map optimize START");
    log!(logger, "Session map optimize END");
    Ok(unoptimized_map)
}
fn apply_optimizations(
    cu: &mut ConnectorUnphased,
    comm: &mut ConnectorCommunication,
    session_info: SessionInfo,
) -> Result<(), ConnectError> {
    let SessionInfo {
        proto_components,
        port_info,
        serde_proto_description,
        endpoint_incoming_to_getter,
    } = session_info;
    // TODO some info which should be read-only can be mutated with the current scheme
    cu.port_info = port_info;
    cu.proto_components = proto_components;
    cu.proto_description = serde_proto_description.0;
    for (ee, getter) in comm
        .endpoint_manager
        .net_endpoint_store
        .endpoint_exts
        .iter_mut()
        .zip(endpoint_incoming_to_getter)
    {
        ee.getter_for_incoming = getter;
    }
    Ok(())
}
