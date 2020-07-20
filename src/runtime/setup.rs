use crate::common::*;
use crate::runtime::*;

impl Connector {
    pub fn new(
        mut logger: Box<dyn Logger>,
        proto_description: Arc<ProtocolDescription>,
        connector_id: ConnectorId,
    ) -> Self {
        log!(&mut *logger, "Created with connector_id {:?}", connector_id);
        Self {
            unphased: ConnectorUnphased {
                proto_description,
                proto_components: Default::default(),
                inner: ConnectorUnphasedInner {
                    logger,
                    id_manager: IdManager::new(connector_id),
                    native_ports: Default::default(),
                    port_info: Default::default(),
                },
            },
            phased: ConnectorPhased::Setup(Box::new(ConnectorSetup {
                net_endpoint_setups: Default::default(),
                udp_endpoint_setups: Default::default(),
            })),
        }
    }
    pub fn new_udp_port(
        &mut self,
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
    ) -> Result<[PortId; 2], WrongStateError> {
        let Self { unphased: cu, phased } = self;
        match phased {
            ConnectorPhased::Communication(..) => Err(WrongStateError),
            ConnectorPhased::Setup(setup) => {
                let udp_index = setup.udp_endpoint_setups.len();
                let mut npid = || cu.inner.id_manager.new_port_id();
                let [nin, nout, uin, uout] = [npid(), npid(), npid(), npid()];
                cu.inner.native_ports.insert(nin);
                cu.inner.native_ports.insert(nout);
                cu.inner.port_info.polarities.insert(nin, Getter);
                cu.inner.port_info.polarities.insert(nout, Putter);
                cu.inner.port_info.polarities.insert(uin, Getter);
                cu.inner.port_info.polarities.insert(uout, Putter);
                cu.inner.port_info.peers.insert(nin, uout);
                cu.inner.port_info.peers.insert(nout, uin);
                cu.inner.port_info.peers.insert(uin, nout);
                cu.inner.port_info.peers.insert(uout, nin);
                cu.inner.port_info.routes.insert(nin, Route::LocalComponent(ComponentId::Native));
                cu.inner.port_info.routes.insert(nout, Route::LocalComponent(ComponentId::Native));
                cu.inner.port_info.routes.insert(uin, Route::UdpEndpoint { index: udp_index });
                cu.inner.port_info.routes.insert(uout, Route::UdpEndpoint { index: udp_index });
                setup.udp_endpoint_setups.push(UdpEndpointSetup {
                    local_addr,
                    peer_addr,
                    getter_for_incoming: nin,
                });
                Ok([nout, nin])
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
                let local_port = cu.inner.id_manager.new_port_id();
                cu.inner.native_ports.insert(local_port);
                // {polarity, route} known. {peer} unknown.
                cu.inner.port_info.polarities.insert(local_port, polarity);
                cu.inner
                    .port_info
                    .routes
                    .insert(local_port, Route::LocalComponent(ComponentId::Native));
                log!(
                    cu.inner.logger,
                    "Added net port {:?} with polarity {:?} addr {:?} endpoint_polarity {:?}",
                    local_port,
                    polarity,
                    &sock_addr,
                    endpoint_polarity
                );
                setup.net_endpoint_setups.push(NetEndpointSetup {
                    sock_addr,
                    endpoint_polarity,
                    getter_for_incoming: local_port,
                });
                Ok(local_port)
            }
        }
    }
    pub fn connect(&mut self, timeout: Option<Duration>) -> Result<(), ConnectError> {
        use ConnectError as Ce;
        let Self { unphased: cu, phased } = self;
        match &phased {
            ConnectorPhased::Communication { .. } => {
                log!(cu.inner.logger, "Call to connecting in connected state");
                Err(Ce::AlreadyConnected)
            }
            ConnectorPhased::Setup(setup) => {
                log!(cu.inner.logger, "~~~ CONNECT called timeout {:?}", timeout);
                let deadline = timeout.map(|to| Instant::now() + to);
                // connect all endpoints in parallel; send and receive peer ids through ports
                let mut endpoint_manager = new_endpoint_manager(
                    &mut *cu.inner.logger,
                    &setup.net_endpoint_setups,
                    &setup.udp_endpoint_setups,
                    &mut cu.inner.port_info,
                    &deadline,
                )?;
                log!(
                    cu.inner.logger,
                    "Successfully connected {} endpoints",
                    endpoint_manager.net_endpoint_store.endpoint_exts.len()
                );
                // leader election and tree construction
                let neighborhood = init_neighborhood(
                    cu.inner.id_manager.connector_id,
                    &mut *cu.inner.logger,
                    &mut endpoint_manager,
                    &deadline,
                )?;
                log!(cu.inner.logger, "Successfully created neighborhood {:?}", &neighborhood);
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
                log!(cu.inner.logger, "connect() finished. setup phase complete");
                self.phased = ConnectorPhased::Communication(Box::new(comm));
                Ok(())
            }
        }
    }
}
fn new_endpoint_manager(
    logger: &mut dyn Logger,
    net_endpoint_setups: &[NetEndpointSetup],
    udp_endpoint_setups: &[UdpEndpointSetup],
    port_info: &mut PortInfo,
    deadline: &Option<Instant>,
) -> Result<EndpointManager, ConnectError> {
    ////////////////////////////////////////////
    use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
    use ConnectError as Ce;
    const BOTH: Interest = Interest::READABLE.add(Interest::WRITABLE);
    const WAKER_PERIOD: Duration = Duration::from_millis(300);
    struct WakerState {
        continue_signal: AtomicBool,
        waker: mio::Waker,
    }
    impl WakerState {
        fn waker_loop(&self) {
            while self.continue_signal.load(SeqCst) {
                std::thread::sleep(WAKER_PERIOD);
                let _ = self.waker.wake();
            }
        }
        fn waker_stop(&self) {
            self.continue_signal.store(false, SeqCst);
            // TODO keep waker registered?
        }
    }
    struct Todo {
        // becomes completed once sent_local_port && recv_peer_port.is_some()
        // we send local port if we haven't already and we receive a writable event
        // we recv peer port if we haven't already and we receive a readbale event
        todo_endpoint: TodoEndpoint,
        endpoint_setup: NetEndpointSetup,
        sent_local_port: bool,          // true <-> I've sent my local port
        recv_peer_port: Option<PortId>, // Some(..) <-> I've received my peer's port
    }
    struct UdpTodo {
        // becomes completed once we receive our first writable event
        getter_for_incoming: PortId,
        sock: UdpSocket,
    }
    enum TodoEndpoint {
        Accepting(TcpListener),
        NetEndpoint(NetEndpoint),
    }
    ////////////////////////////////////////////

    // 1. Start to construct EndpointManager
    let mut waker_state: Option<Arc<WakerState>> = None;
    let mut poll = Poll::new().map_err(|_| Ce::PollInitFailed)?;
    let mut events =
        Events::with_capacity((net_endpoint_setups.len() + udp_endpoint_setups.len()) * 2 + 4);
    let [mut net_polled_undrained, udp_polled_undrained] = [VecSet::default(), VecSet::default()];
    let mut delayed_messages = vec![];

    // 2. Create net/udp TODOs, each already registered with poll
    let mut net_todos = net_endpoint_setups
        .iter()
        .enumerate()
        .map(|(index, endpoint_setup)| {
            let token = TokenTarget::NetEndpoint { index }.into();
            log!(logger, "Net endpoint {} beginning setup with {:?}", index, &endpoint_setup);
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
                sent_local_port: false,
                recv_peer_port: None,
                endpoint_setup: endpoint_setup.clone(),
            })
        })
        .collect::<Result<Vec<Todo>, ConnectError>>()?;
    let udp_todos = udp_endpoint_setups
        .iter()
        .enumerate()
        .map(|(index, endpoint_setup)| {
            let mut sock = UdpSocket::bind(endpoint_setup.local_addr)
                .map_err(|_| Ce::BindFailed(endpoint_setup.local_addr))?;
            sock.connect(endpoint_setup.peer_addr)
                .map_err(|_| Ce::UdpConnectFailed(endpoint_setup.peer_addr))?;
            poll.registry()
                .register(&mut sock, TokenTarget::UdpEndpoint { index }.into(), Interest::WRITABLE)
                .unwrap();
            Ok(UdpTodo { sock, getter_for_incoming: endpoint_setup.getter_for_incoming })
        })
        .collect::<Result<Vec<UdpTodo>, ConnectError>>()?;

    // Initially, (1) no net connections have failed, and (2) all udp and net endpoint setups are incomplete
    let mut net_connect_retry_later: HashSet<usize> = Default::default();
    let mut setup_incomplete: HashSet<TokenTarget> = {
        let net_todo_targets_iter =
            (0..net_todos.len()).map(|index| TokenTarget::NetEndpoint { index });
        let udp_todo_targets_iter =
            (0..udp_todos.len()).map(|index| TokenTarget::UdpEndpoint { index });
        net_todo_targets_iter.chain(udp_todo_targets_iter).collect()
    };
    // progress by reacting to poll events. continue until every endpoint is set up
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
                        net_connect_retry_later.iter()
                    );
                    assert!(waker_state.is_some());
                    for net_index in net_connect_retry_later.drain() {
                        let net_todo = &mut net_todos[net_index];
                        log!(
                            logger,
                            "Restarting connection with endpoint {:?} {:?}",
                            net_index,
                            net_todo.endpoint_setup.sock_addr
                        );
                        match &mut net_todo.todo_endpoint {
                            TodoEndpoint::NetEndpoint(endpoint) => {
                                let mut new_stream =
                                    TcpStream::connect(net_todo.endpoint_setup.sock_addr)
                                        .expect("mio::TcpStream connect should not fail!");
                                std::mem::swap(&mut endpoint.stream, &mut new_stream);
                                let token = TokenTarget::NetEndpoint { index: net_index }.into();
                                poll.registry()
                                    .register(&mut endpoint.stream, token, BOTH)
                                    .unwrap();
                            }
                            _ => unreachable!(),
                        }
                    }
                }
                TokenTarget::UdpEndpoint { index } => {
                    if !setup_incomplete.contains(&token_target) {
                        // spurious wakeup. this endpoint has already been set up!
                        continue;
                    }
                    let udp_todo: &UdpTodo = &udp_todos[index];
                    if event.is_error() {
                        return Err(Ce::BindFailed(udp_todo.sock.local_addr().unwrap()));
                    }
                    setup_incomplete.remove(&token_target);
                }
                TokenTarget::NetEndpoint { index } => {
                    let net_todo = &mut net_todos[index];
                    if let TodoEndpoint::Accepting(listener) = &mut net_todo.todo_endpoint {
                        // FIRST try complete this connection
                        match listener.accept() {
                            Err(e) if would_block(&e) => continue, // spurious wakeup
                            Err(_) => {
                                log!(logger, "accept() failure on index {}", index);
                                return Err(Ce::AcceptFailed(listener.local_addr().unwrap()));
                            }
                            Ok((mut stream, peer_addr)) => {
                                // successfully accepted the active peer
                                // reusing the token, but now for the stream and not the listener
                                poll.registry().deregister(listener).unwrap();
                                poll.registry().register(&mut stream, token, BOTH).unwrap();
                                log!(
                                    logger,
                                    "Endpoint[{}] accepted a connection from {:?}",
                                    index,
                                    peer_addr
                                );
                                let net_endpoint = NetEndpoint { stream, inbox: vec![] };
                                net_todo.todo_endpoint = TodoEndpoint::NetEndpoint(net_endpoint);
                            }
                        }
                    }
                    if let TodoEndpoint::NetEndpoint(net_endpoint) = &mut net_todo.todo_endpoint {
                        if event.is_error() {
                            if net_todo.endpoint_setup.endpoint_polarity
                                == EndpointPolarity::Passive
                            {
                                // right now you cannot retry an acceptor. return failure
                                return Err(Ce::AcceptFailed(
                                    net_endpoint.stream.local_addr().unwrap(),
                                ));
                            }
                            // this actively-connecting endpoint failed to connect!
                            if net_connect_retry_later.insert(index) {
                                log!(
                                    logger,
                                    "Connection failed for {:?}. List is {:?}",
                                    index,
                                    net_connect_retry_later.iter()
                                );
                                poll.registry().deregister(&mut net_endpoint.stream).unwrap();
                            } else {
                                // spurious wakeup. already scheduled to retry connect later
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
                                std::thread::spawn(move || moved_arc.waker_loop());
                            }
                            continue;
                        }
                        // event wasn't ERROR
                        if net_connect_retry_later.contains(&index) {
                            // spurious wakeup. already scheduled to retry connect later
                            continue;
                        }
                        if !setup_incomplete.contains(&token_target) {
                            // spurious wakeup. this endpoint has already been completed!
                            if event.is_readable() {
                                net_polled_undrained.insert(index);
                            }
                            continue;
                        }
                        let local_polarity = *port_info
                            .polarities
                            .get(&net_todo.endpoint_setup.getter_for_incoming)
                            .unwrap();
                        if event.is_writable() && !net_todo.sent_local_port {
                            // can write and didn't send setup msg yet? Do so!
                            let msg = Msg::SetupMsg(SetupMsg::MyPortInfo(MyPortInfo {
                                polarity: local_polarity,
                                port: net_todo.endpoint_setup.getter_for_incoming,
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
                            net_todo.sent_local_port = true;
                        }
                        if event.is_readable() && net_todo.recv_peer_port.is_none() {
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
                                            net_todo.endpoint_setup.getter_for_incoming,
                                        ));
                                    }
                                    net_todo.recv_peer_port = Some(peer_info.port);
                                    // 1. finally learned the peer of this port!
                                    port_info.peers.insert(
                                        net_todo.endpoint_setup.getter_for_incoming,
                                        peer_info.port,
                                    );
                                    // 2. learned the info of this peer port
                                    port_info.polarities.insert(peer_info.port, peer_info.polarity);
                                    port_info.peers.insert(
                                        peer_info.port,
                                        net_todo.endpoint_setup.getter_for_incoming,
                                    );
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
                        if net_todo.sent_local_port && net_todo.recv_peer_port.is_some() {
                            // yes! connected, sent my info and received peer's info
                            setup_incomplete.remove(&token_target);
                            log!(logger, "endpoint[{}] is finished!", index);
                        }
                    }
                }
            }
        }
        events.clear();
    }
    log!(logger, "Endpoint setup complete! Cleaning up and building structures");
    if let Some(ws) = waker_state.take() {
        ws.waker_stop();
    }
    let net_endpoint_exts = net_todos
        .into_iter()
        .enumerate()
        .map(|(index, Todo { todo_endpoint, endpoint_setup, .. })| NetEndpointExt {
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
            getter_for_incoming: endpoint_setup.getter_for_incoming,
        })
        .collect();
    let udp_endpoint_exts = udp_todos
        .into_iter()
        .enumerate()
        .map(|(index, udp_todo)| {
            let UdpTodo { mut sock, getter_for_incoming } = udp_todo;
            let token = TokenTarget::UdpEndpoint { index }.into();
            poll.registry().reregister(&mut sock, token, Interest::READABLE).unwrap();
            UdpEndpointExt {
                sock,
                outgoing_payloads: Default::default(),
                incoming_round_spec_var: None,
                getter_for_incoming,
                incoming_payloads: Default::default(),
            }
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
    log!(cu.inner.logger, "Beginning session optimization");
    // populate session_info_map from a message per child
    let mut unoptimized_map: HashMap<ConnectorId, SessionInfo> = Default::default();
    let mut awaiting: HashSet<usize> = comm.neighborhood.children.iter().copied().collect();
    comm.endpoint_manager.undelay_all();
    while !awaiting.is_empty() {
        log!(
            cu.inner.logger,
            "Session gather loop. awaiting info from children {:?}...",
            awaiting.iter()
        );
        let (recv_index, msg) =
            comm.endpoint_manager.try_recv_any_setup(&mut *cu.inner.logger, deadline)?;
        log!(cu.inner.logger, "Received from index {:?} msg {:?}", &recv_index, &msg);
        match msg {
            S(Sm::SessionGather { unoptimized_map: child_unoptimized_map }) => {
                if !awaiting.remove(&recv_index) {
                    log!(
                        cu.inner.logger,
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
                log!(cu.inner.logger, "discarding old message {:?} during election", msg);
            }
            msg @ S(Sm::SessionScatter { .. }) => {
                log!(
                    cu.inner.logger,
                    "Endpoint {:?} sent unexpected scatter! {:?} I've not contributed yet!",
                    recv_index,
                    &msg
                );
                return Err(Ce::SetupAlgMisbehavior);
            }
            msg @ Msg::CommMsg(..) => {
                log!(cu.inner.logger, "delaying msg {:?} during session optimization", msg);
                comm.endpoint_manager.delayed_messages.push((recv_index, msg));
            }
        }
    }
    log!(
        cu.inner.logger,
        "Gathered all children's maps. ConnectorId set is... {:?}",
        unoptimized_map.keys()
    );
    let my_session_info = SessionInfo {
        port_info: cu.inner.port_info.clone(),
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
    unoptimized_map.insert(cu.inner.id_manager.connector_id, my_session_info);
    log!(
        cu.inner.logger,
        "Inserting my own info. Unoptimized subtree map is {:?}",
        &unoptimized_map
    );

    // acquire the optimized info...
    let optimized_map = if let Some(parent) = comm.neighborhood.parent {
        // ... as a message from my parent
        log!(cu.inner.logger, "Forwarding gathered info to parent {:?}", parent);
        let msg = S(Sm::SessionGather { unoptimized_map });
        comm.endpoint_manager.send_to_setup(parent, &msg)?;
        'scatter_loop: loop {
            log!(
                cu.inner.logger,
                "Session scatter recv loop. awaiting info from children {:?}...",
                awaiting.iter()
            );
            let (recv_index, msg) =
                comm.endpoint_manager.try_recv_any_setup(&mut *cu.inner.logger, deadline)?;
            log!(cu.inner.logger, "Received from index {:?} msg {:?}", &recv_index, &msg);
            match msg {
                S(Sm::SessionScatter { optimized_map }) => {
                    if recv_index != parent {
                        log!(cu.inner.logger, "I expected the scatter from my parent only!");
                        return Err(Ce::SetupAlgMisbehavior);
                    }
                    break 'scatter_loop optimized_map;
                }
                msg @ Msg::CommMsg { .. } => {
                    log!(cu.inner.logger, "delaying msg {:?} during scatter recv", msg);
                    comm.endpoint_manager.delayed_messages.push((recv_index, msg));
                }
                msg @ S(Sm::SessionGather { .. })
                | msg @ S(Sm::YouAreMyParent)
                | msg @ S(Sm::MyPortInfo(..))
                | msg @ S(Sm::LeaderAnnounce { .. })
                | msg @ S(Sm::LeaderWave { .. }) => {
                    log!(cu.inner.logger, "discarding old message {:?} during election", msg);
                }
            }
        }
    } else {
        // by computing it myself
        log!(cu.inner.logger, "I am the leader! I will optimize this session");
        leader_session_map_optimize(&mut *cu.inner.logger, unoptimized_map)?
    };
    log!(
        cu.inner.logger,
        "Optimized info map is {:?}. Sending to children {:?}",
        &optimized_map,
        comm.neighborhood.children.iter()
    );
    log!(cu.inner.logger, "All session info dumped!: {:#?}", &optimized_map);
    let optimized_info =
        optimized_map.get(&cu.inner.id_manager.connector_id).expect("HEY NO INFO FOR ME?").clone();
    let msg = S(Sm::SessionScatter { optimized_map });
    for &child in comm.neighborhood.children.iter() {
        comm.endpoint_manager.send_to_setup(child, &msg)?;
    }
    apply_optimizations(cu, comm, optimized_info)?;
    log!(cu.inner.logger, "Session optimizations applied");
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
    cu.inner.port_info = port_info;
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
