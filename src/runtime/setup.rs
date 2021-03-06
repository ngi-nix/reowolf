use crate::common::*;
use crate::runtime::*;

impl TokenTarget {
    // subdivides the domain of usize into
    // [NET_ENDPOINT][UDP_ENDPOINT  ]
    // ^0            ^usize::MAX/2   ^usize::MAX
    const HALFWAY_INDEX: usize = usize::MAX / 2;
}
impl From<Token> for TokenTarget {
    fn from(Token(index): Token) -> Self {
        if let Some(shifted) = index.checked_sub(Self::HALFWAY_INDEX) {
            TokenTarget::UdpEndpoint { index: shifted }
        } else {
            TokenTarget::NetEndpoint { index }
        }
    }
}
impl Into<Token> for TokenTarget {
    fn into(self) -> Token {
        match self {
            TokenTarget::UdpEndpoint { index } => Token(index + Self::HALFWAY_INDEX),
            TokenTarget::NetEndpoint { index } => Token(index),
        }
    }
}
impl Connector {
    /// Create a new connector structure with the given protocol description (via Arc to facilitate sharing).
    /// The resulting connector will start in the setup phase, and cannot be used for communication until the
    /// `connect` procedure completes.
    /// # Safety
    /// The correctness of the system's underlying distributed algorithms requires that no two
    /// connectors have the same ID. If the user does not know the identifiers of other connectors in the
    /// system, it is advised to guess it using Connector::random_id (relying on the exceptionally low probability of an error).
    /// Sessions with duplicate connector identifiers will not result in any memory unsafety, but cannot be guaranteed
    /// to preserve their configured protocols.
    /// Fortunately, in most realistic cases, the presence of duplicate connector identifiers will result in an
    /// error during `connect`, observed as a peer misbehaving.
    pub fn new(
        mut logger: Box<dyn Logger>,
        proto_description: Arc<ProtocolDescription>,
        connector_id: ConnectorId,
    ) -> Self {
        log!(&mut *logger, "Created with connector_id {:?}", connector_id);
        let mut id_manager = IdManager::new(connector_id);
        let native_component_id = id_manager.new_component_id();
        Self {
            unphased: ConnectorUnphased {
                proto_description,
                proto_components: Default::default(),
                logger,
                native_component_id,
                ips: IdAndPortState { id_manager, port_info: Default::default() },
            },
            phased: ConnectorPhased::Setup(Box::new(ConnectorSetup {
                net_endpoint_setups: Default::default(),
                udp_endpoint_setups: Default::default(),
            })),
        }
    }

    /// Conceptually, this returning [p0, g1] is sugar for:
    /// 1. create port pair [p0, g0]
    /// 2. create port pair [p1, g1]
    /// 3. create udp component with interface of moved ports [p1, g0]
    /// 4. return [p0, g1]
    pub fn new_udp_mediator_component(
        &mut self,
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
    ) -> Result<[PortId; 2], WrongStateError> {
        let Self { unphased: cu, phased } = self;
        match phased {
            ConnectorPhased::Communication(..) => Err(WrongStateError),
            ConnectorPhased::Setup(setup) => {
                let udp_index = setup.udp_endpoint_setups.len();
                let udp_cid = cu.ips.id_manager.new_component_id();
                // allocates 4 new port identifiers, two for each logical channel,
                // one channel per direction (into and out of the component)
                let mut npid = || cu.ips.id_manager.new_port_id();
                let [nin, nout, uin, uout] = [npid(), npid(), npid(), npid()];
                // allocate the native->udp_mediator channel's ports
                cu.ips.port_info.map.insert(
                    nout,
                    PortInfo {
                        route: Route::LocalComponent,
                        polarity: Putter,
                        peer: Some(uin),
                        owner: cu.native_component_id,
                    },
                );
                cu.ips.port_info.map.insert(
                    uin,
                    PortInfo {
                        route: Route::UdpEndpoint { index: udp_index },
                        polarity: Getter,
                        peer: Some(uin),
                        owner: udp_cid,
                    },
                );
                // allocate the udp_mediator->native channel's ports
                cu.ips.port_info.map.insert(
                    uout,
                    PortInfo {
                        route: Route::UdpEndpoint { index: udp_index },
                        polarity: Putter,
                        peer: Some(uin),
                        owner: udp_cid,
                    },
                );
                cu.ips.port_info.map.insert(
                    nin,
                    PortInfo {
                        route: Route::LocalComponent,
                        polarity: Getter,
                        peer: Some(uout),
                        owner: cu.native_component_id,
                    },
                );
                // allocate the two ports owned by the UdpMediator component
                // Remember to setup this UdpEndpoint setup during `connect` later.
                setup.udp_endpoint_setups.push(UdpEndpointSetup {
                    local_addr,
                    peer_addr,
                    getter_for_incoming: nin,
                });

                // update owned sets
                cu.ips
                    .port_info
                    .owned
                    .entry(cu.native_component_id)
                    .or_default()
                    .extend([nin, nout].iter().copied());
                cu.ips.port_info.owned.insert(udp_cid, maplit::hashset! {uin, uout});
                // Return the native's output, input port pair
                Ok([nout, nin])
            }
        }
    }

    /// Adds a "dangling" port to the connector in the setup phase,
    /// to be formed into channel during the connect procedure with the given
    /// transport layer information.
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
                // allocate a single dangling port with a `None` peer (for now)
                let new_pid = cu.ips.id_manager.new_port_id();
                cu.ips.port_info.map.insert(
                    new_pid,
                    PortInfo {
                        route: Route::LocalComponent,
                        peer: None,
                        owner: cu.native_component_id,
                        polarity,
                    },
                );
                log!(
                    cu.logger,
                    "Added net port {:?} with polarity {:?} addr {:?} endpoint_polarity {:?}",
                    new_pid,
                    polarity,
                    &sock_addr,
                    endpoint_polarity
                );
                // Remember to setup this NetEndpoint setup during `connect` later.
                setup.net_endpoint_setups.push(NetEndpointSetup {
                    sock_addr,
                    endpoint_polarity,
                    getter_for_incoming: new_pid,
                });
                // update owned set
                cu.ips.port_info.owned.entry(cu.native_component_id).or_default().insert(new_pid);
                Ok(new_pid)
            }
        }
    }

    /// Finalizes the connector's setup procedure and forms a distributed system with
    /// all other connectors reachable through network channels. This procedure represents
    /// a synchronization barrier, and upon successful return, the connector can no longer add new network ports,
    /// but is ready to begin the first communication round.
    /// Initially, the connector has a singleton set of _batches_, the only element of which is empty.
    /// This single element starts off selected. The selected batch is modified with `put` and `get`,
    /// and new batches are added and selected with `next_batch`. See `sync` for an explanation of the
    /// purpose of these batches.
    pub fn connect(&mut self, timeout: Option<Duration>) -> Result<(), ConnectError> {
        use ConnectError as Ce;
        let Self { unphased: cu, phased } = self;
        match &phased {
            ConnectorPhased::Communication { .. } => {
                log!(cu.logger, "Call to connecting in connected state");
                Err(Ce::AlreadyConnected)
            }
            ConnectorPhased::Setup(setup) => {
                // Idea: Clone `self.unphased`, and then pass the replica to
                // `connect_inner` to do the work, attempting to create a new connector structure
                // in connected state without encountering any errors.
                // If anything goes wrong during `connect_inner`, we simply keep the original `cu`.

                // Ideally, we'd simply clone `cu` in its entirety.
                // However, it isn't clonable, because of the pesky logger.
                // Solution: the original and clone ConnectorUnphased structures
                // 'share' the original logger by using `mem::swap` strategically to pass a dummy back and forth,
                // such that the real logger is wherever we need it to be without violating any invariants.
                let mut cu_clone = ConnectorUnphased {
                    logger: Box::new(DummyLogger),
                    proto_components: cu.proto_components.clone(),
                    native_component_id: cu.native_component_id.clone(),
                    ips: cu.ips.clone(),
                    proto_description: cu.proto_description.clone(),
                };
                // cu has REAL logger...
                std::mem::swap(&mut cu.logger, &mut cu_clone.logger);
                // ... cu_clone has REAL logger.
                match Self::connect_inner(cu_clone, setup, timeout) {
                    Ok(connected_connector) => {
                        *self = connected_connector;
                        Ok(())
                    }
                    Err((err, mut logger)) => {
                        // Put the original logger back in place (in self.unphased, AKA `cu`).
                        // cu_clone has REAL logger...
                        std::mem::swap(&mut cu.logger, &mut logger);
                        // ... cu has REAL logger.
                        Err(err)
                    }
                }
            }
        }
    }

    // Given an immutable setup structure, and my own (cloned) ConnetorUnphased,
    // attempt to complete the setup procedure and return a new connector in Connected state.
    // If anything goes wrong, throw everything in the bin, except for the Logger, which is
    // the only structure that sees lasting effects of the failed attempt.
    fn connect_inner(
        mut cu: ConnectorUnphased,
        setup: &ConnectorSetup,
        timeout: Option<Duration>,
    ) -> Result<Self, (ConnectError, Box<dyn Logger>)> {
        log!(cu.logger, "~~~ CONNECT called timeout {:?}", timeout);
        let deadline = timeout.map(|to| Instant::now() + to);
        // `try_complete` is a helper function, which DOES NOT own `cu`, and returns ConnectError on err.
        // This outer function takes its output and wraps it alongside `cu` (which it owns)
        // as appropriate for Err(...) and OK(...) cases.
        let mut try_complete = || {
            // connect all endpoints in parallel; send and receive peer ids through ports
            let mut endpoint_manager = setup_endpoints_and_pair_ports(
                &mut *cu.logger,
                &setup.net_endpoint_setups,
                &setup.udp_endpoint_setups,
                &mut cu.ips.port_info,
                &deadline,
            )?;
            log!(
                cu.logger,
                "Successfully connected {} endpoints. info now {:#?} {:#?}",
                endpoint_manager.net_endpoint_store.endpoint_exts.len(),
                &cu.ips.port_info,
                &endpoint_manager,
            );
            // leader election and tree construction. Learn our role in the consensus tree,
            // from learning who are our children/parents (neighbors) in the consensus tree.
            let neighborhood = init_neighborhood(
                cu.ips.id_manager.connector_id,
                &mut *cu.logger,
                &mut endpoint_manager,
                &deadline,
            )?;
            log!(cu.logger, "Successfully created neighborhood {:?}", &neighborhood);
            // Put it all together with an initial round index of zero.
            let comm = ConnectorCommunication {
                round_index: 0,
                endpoint_manager,
                neighborhood,
                native_batches: vec![Default::default()],
                round_result: Ok(None), // no previous round yet
            };
            log!(cu.logger, "connect() finished. setup phase complete");
            Ok(comm)
        };
        match try_complete() {
            Ok(comm) => {
                Ok(Self { unphased: cu, phased: ConnectorPhased::Communication(Box::new(comm)) })
            }
            Err(err) => Err((err, cu.logger)),
        }
    }
}

// Given a set of net_ and udp_ endpoints to setup,
// port information to flesh out (by discovering peers through channels)
// and a deadline in which to do it,
// try to return:
// - An EndpointManager, containing all the set up endpoints
// - new information about ports acquired through the newly-created channels
fn setup_endpoints_and_pair_ports(
    logger: &mut dyn Logger,
    net_endpoint_setups: &[NetEndpointSetup],
    udp_endpoint_setups: &[UdpEndpointSetup],
    port_info: &mut PortInfoMap,
    deadline: &Option<Instant>,
) -> Result<EndpointManager, ConnectError> {
    use ConnectError as Ce;
    const BOTH: Interest = Interest::READABLE.add(Interest::WRITABLE);
    const RETRY_PERIOD: Duration = Duration::from_millis(200);

    // The data for a net endpoint's setup in progress
    struct NetTodo {
        // becomes completed once sent_local_port && recv_peer_port.is_some()
        // we send local port if we haven't already and we receive a writable event
        // we recv peer port if we haven't already and we receive a readbale event
        todo_endpoint: NetTodoEndpoint,
        endpoint_setup: NetEndpointSetup,
        sent_local_port: bool,          // true <-> I've sent my local port
        recv_peer_port: Option<PortId>, // Some(..) <-> I've received my peer's port
    }

    // The data for a udp endpoint's setup in progress
    struct UdpTodo {
        // becomes completed once we receive our first writable event
        getter_for_incoming: PortId,
        sock: UdpSocket,
    }

    // Substructure of `NetTodo`, which represents the endpoint itself
    enum NetTodoEndpoint {
        Accepting(TcpListener),       // awaiting it's peer initiating the connection
        PeerInfoRecving(NetEndpoint), // awaiting info about peer port through the channel
    }
    ////////////////////////////////////////////

    // Start to construct our return values
    let mut poll = Poll::new().map_err(|_| Ce::PollInitFailed)?;
    let mut events =
        Events::with_capacity((net_endpoint_setups.len() + udp_endpoint_setups.len()) * 2 + 4);
    let [mut net_polled_undrained, udp_polled_undrained] = [VecSet::default(), VecSet::default()];
    let mut delayed_messages = vec![];
    let mut last_retry_at = Instant::now();
    let mut io_byte_buffer = IoByteBuffer::default();

    // Create net/udp todo structures, each already registered with poll
    let mut net_todos = net_endpoint_setups
        .iter()
        .enumerate()
        .map(|(index, endpoint_setup)| {
            let token = TokenTarget::NetEndpoint { index }.into();
            log!(logger, "Net endpoint {} beginning setup with {:?}", index, &endpoint_setup);
            let todo_endpoint = if let EndpointPolarity::Active = endpoint_setup.endpoint_polarity {
                let mut stream = TcpStream::connect(endpoint_setup.sock_addr)
                    .map_err(|_| Ce::TcpInvalidConnect(endpoint_setup.sock_addr))?;
                poll.registry().register(&mut stream, token, BOTH).unwrap();
                NetTodoEndpoint::PeerInfoRecving(NetEndpoint { stream, inbox: vec![] })
            } else {
                let mut listener = TcpListener::bind(endpoint_setup.sock_addr)
                    .map_err(|_| Ce::BindFailed(endpoint_setup.sock_addr))?;
                poll.registry().register(&mut listener, token, BOTH).unwrap();
                NetTodoEndpoint::Accepting(listener)
            };
            Ok(NetTodo {
                todo_endpoint,
                sent_local_port: false,
                recv_peer_port: None,
                endpoint_setup: endpoint_setup.clone(),
            })
        })
        .collect::<Result<Vec<NetTodo>, ConnectError>>()?;
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

    // Initially no net connections have failed, and all udp and net endpoint setups are incomplete
    let mut net_connect_to_retry: HashSet<usize> = Default::default();
    let mut setup_incomplete: HashSet<TokenTarget> = {
        let net_todo_targets_iter =
            (0..net_todos.len()).map(|index| TokenTarget::NetEndpoint { index });
        let udp_todo_targets_iter =
            (0..udp_todos.len()).map(|index| TokenTarget::UdpEndpoint { index });
        net_todo_targets_iter.chain(udp_todo_targets_iter).collect()
    };
    // progress by reacting to poll events. continue until every endpoint is set up
    while !setup_incomplete.is_empty() {
        // recompute the timeout for the poll call
        let remaining = match (deadline, net_connect_to_retry.is_empty()) {
            (None, true) => None,
            (None, false) => Some(RETRY_PERIOD),
            (Some(deadline), is_empty) => {
                let dur_to_timeout =
                    deadline.checked_duration_since(Instant::now()).ok_or(Ce::Timeout)?;
                Some(if is_empty { dur_to_timeout } else { dur_to_timeout.min(RETRY_PERIOD) })
            }
        };
        // block until either
        // (a) `events` has been populated with 1+ elements
        // (b) timeout elapses, or
        // (c) RETRY_PERIOD elapses
        poll.poll(&mut events, remaining).map_err(|_| Ce::PollFailed)?;
        if last_retry_at.elapsed() > RETRY_PERIOD {
            // Retry all net connections and reset `last_retry_at`
            last_retry_at = Instant::now();
            for net_index in net_connect_to_retry.drain() {
                // Restart connect procedure for this net endpoint
                let net_todo = &mut net_todos[net_index];
                log!(
                    logger,
                    "Restarting connection with endpoint {:?} {:?}",
                    net_index,
                    net_todo.endpoint_setup.sock_addr
                );
                match &mut net_todo.todo_endpoint {
                    NetTodoEndpoint::PeerInfoRecving(endpoint) => {
                        let mut new_stream = TcpStream::connect(net_todo.endpoint_setup.sock_addr)
                            .expect("mio::TcpStream connect should not fail!");
                        std::mem::swap(&mut endpoint.stream, &mut new_stream);
                        let token = TokenTarget::NetEndpoint { index: net_index }.into();
                        poll.registry().register(&mut endpoint.stream, token, BOTH).unwrap();
                    }
                    _ => unreachable!(),
                }
            }
        }
        for event in events.iter() {
            let token = event.token();
            // figure out which endpoint the event belonged to
            let token_target = TokenTarget::from(token);
            match token_target {
                TokenTarget::UdpEndpoint { index } => {
                    // UdpEndpoints are easy to complete.
                    // Their setup event just has to succeed without error
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
                    // NetEndpoints are complex to complete,
                    // they must accept/connect to their peer,
                    // and then exchange port info successfully
                    let net_todo = &mut net_todos[index];
                    if let NetTodoEndpoint::Accepting(listener) = &mut net_todo.todo_endpoint {
                        // Passive endpoint that will first try accept the peer's connection
                        match listener.accept() {
                            Err(e) if err_would_block(&e) => continue, // spurious wakeup
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
                                net_todo.todo_endpoint =
                                    NetTodoEndpoint::PeerInfoRecving(net_endpoint);
                            }
                        }
                    }
                    // OK now let's try and finish exchanging port info
                    if let NetTodoEndpoint::PeerInfoRecving(net_endpoint) =
                        &mut net_todo.todo_endpoint
                    {
                        if event.is_error() {
                            // event signals some error! :(
                            if net_todo.endpoint_setup.endpoint_polarity
                                == EndpointPolarity::Passive
                            {
                                // breaking as the acceptor is currently unrecoverable
                                return Err(Ce::AcceptFailed(
                                    net_endpoint.stream.local_addr().unwrap(),
                                ));
                            }
                            // this actively-connecting endpoint failed to connect!
                            // We will schedule it for a retry
                            net_connect_to_retry.insert(index);
                            continue;
                        }
                        // event wasn't ERROR
                        if net_connect_to_retry.contains(&index) {
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
                        let local_info = port_info
                            .map
                            .get(&net_todo.endpoint_setup.getter_for_incoming)
                            .expect("Net Setup's getter port info isn't known"); // unreachable
                        if event.is_writable() && !net_todo.sent_local_port {
                            // can write and didn't send setup msg yet? Do so!
                            let _ = net_endpoint.stream.set_nodelay(true);
                            let msg = Msg::SetupMsg(SetupMsg::MyPortInfo(MyPortInfo {
                                owner: local_info.owner,
                                polarity: local_info.polarity,
                                port: net_todo.endpoint_setup.getter_for_incoming,
                            }));
                            net_endpoint
                                .send(&msg, &mut io_byte_buffer)
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
                            // can read and didn't finish recving setup msg yet? Do so!
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
                                    if peer_info.polarity == local_info.polarity {
                                        return Err(ConnectError::PortPeerPolarityMismatch(
                                            net_todo.endpoint_setup.getter_for_incoming,
                                        ));
                                    }
                                    net_todo.recv_peer_port = Some(peer_info.port);
                                    // finally learned the peer of this port!
                                    port_info
                                        .map
                                        .get_mut(&net_todo.endpoint_setup.getter_for_incoming)
                                        .unwrap()
                                        .peer = Some(peer_info.port);
                                    // learned the info of this peer port
                                    port_info.map.entry(peer_info.port).or_insert({
                                        port_info
                                            .owned
                                            .entry(peer_info.owner)
                                            .or_default()
                                            .insert(peer_info.port);
                                        PortInfo {
                                            peer: Some(net_todo.endpoint_setup.getter_for_incoming),
                                            polarity: peer_info.polarity,
                                            owner: peer_info.owner,
                                            route: Route::NetEndpoint { index },
                                        }
                                    });
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
    let net_endpoint_exts = net_todos
        .into_iter()
        .enumerate()
        .map(|(index, NetTodo { todo_endpoint, endpoint_setup, .. })| NetEndpointExt {
            net_endpoint: match todo_endpoint {
                NetTodoEndpoint::PeerInfoRecving(mut net_endpoint) => {
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
                received_this_round: false,
                getter_for_incoming,
            }
        })
        .collect();
    let endpoint_manager = EndpointManager {
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
        io_byte_buffer,
    };
    Ok(endpoint_manager)
}

// Given a fully-formed endpoint manager,
// construct the consensus tree with:
// 1. decentralized leader election
// 2. centralized tree construction
fn init_neighborhood(
    connector_id: ConnectorId,
    logger: &mut dyn Logger,
    em: &mut EndpointManager,
    deadline: &Option<Instant>,
) -> Result<Neighborhood, ConnectError> {
    use {ConnectError as Ce, Msg::SetupMsg as S, SetupMsg as Sm};

    // storage structure for the state of a distributed wave
    // (for readability)
    #[derive(Debug)]
    struct WaveState {
        parent: Option<usize>,
        leader: ConnectorId,
    }

    // kick off a leader-election wave rooted at myself
    // given the desired wave information
    // (e.g. don't inform my parent if they exist)
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
                    // A neighbor explicitly tells me who is the leader
                    // they become my parent, and I adopt their announced leader
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
                msg @ Msg::CommMsg { .. } => {
                    log!(logger, "delaying msg {:?} during election algorithm", msg);
                    em.delayed_messages.push((recv_index, msg));
                }
            }
        }
    };

    // starting algorithm 2. Send a message to every neighbor
    // namely, send "YouAreMyParent" to parent (if they exist),
    // and LeaderAnnounce to everyone else
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
    // Receive one message from each neighbor to learn
    // whether they consider me their parent or not.
    let mut children = vec![];
    em.undelay_all();
    while !awaiting.is_empty() {
        log!(logger, "Tree construction_loop loop. awaiting {:?}...", awaiting.iter());
        let (recv_index, msg) = em.try_recv_any_setup(logger, deadline)?;
        log!(logger, "Received from index {:?} msg {:?}", &recv_index, &msg);
        match msg {
            S(Sm::LeaderAnnounce { .. }) => {
                // `recv_index` is not my child
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
                // `recv_index` is my child
                children.push(recv_index);
            }
            msg @ S(Sm::MyPortInfo(_)) | msg @ S(Sm::LeaderWave { .. }) => {
                log!(logger, "discarding old message {:?} during election", msg);
            }
            msg @ Msg::CommMsg { .. } => {
                log!(logger, "delaying msg {:?} during election", msg);
                em.delayed_messages.push((recv_index, msg));
            }
        }
    }
    // Neighborhood complete!
    children.shrink_to_fit();
    let neighborhood =
        Neighborhood { parent: election_result.parent, children: VecSet::new(children) };
    log!(logger, "Neighborhood constructed {:?}", &neighborhood);
    Ok(neighborhood)
}
