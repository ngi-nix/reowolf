use super::*;
use crate::common::*;

////////////////
struct BranchingNative {
    branches: HashMap<Predicate, NativeBranch>,
}
#[derive(Clone, Debug)]
struct NativeBranch {
    index: usize,
    gotten: HashMap<PortId, Payload>,
    to_get: HashSet<PortId>,
}
#[derive(Debug)]
struct SolutionStorage {
    old_local: HashSet<Predicate>,
    new_local: HashSet<Predicate>,
    // this pair acts as Route -> HashSet<Predicate> which is friendlier to iteration
    subtree_solutions: Vec<HashSet<Predicate>>,
    subtree_id_to_index: HashMap<Route, usize>,
}
#[derive(Debug)]
struct BranchingProtoComponent {
    ports: HashSet<PortId>,
    branches: HashMap<Predicate, ProtoComponentBranch>,
}
#[derive(Debug, Clone)]
struct ProtoComponentBranch {
    inbox: HashMap<PortId, Payload>,
    state: ComponentState,
}
struct CyclicDrainer<'a, K: Eq + Hash, V> {
    input: &'a mut HashMap<K, V>,
    inner: CyclicDrainInner<'a, K, V>,
}
struct CyclicDrainInner<'a, K: Eq + Hash, V> {
    swap: &'a mut HashMap<K, V>,
    output: &'a mut HashMap<K, V>,
}

////////////////
impl NonsyncProtoContext<'_> {
    pub fn new_component(&mut self, moved_ports: HashSet<PortId>, state: ComponentState) {
        // called by a PROTO COMPONENT. moves its own ports.
        // 1. sanity check: this component owns these ports
        log!(
            self.logger,
            "Component {:?} added new component with state {:?}, moving ports {:?}",
            self.proto_component_id,
            &state,
            &moved_ports
        );
        assert!(self.proto_component_ports.is_subset(&moved_ports));
        // 2. remove ports from old component & update port->route
        let new_id = self.id_manager.new_proto_component_id();
        for port in moved_ports.iter() {
            self.proto_component_ports.remove(port);
            self.port_info
                .routes
                .insert(*port, Route::LocalComponent(LocalComponentId::Proto(new_id)));
        }
        // 3. create a new component
        self.unrun_components.push((new_id, ProtoComponent { state, ports: moved_ports }));
    }
    pub fn new_port_pair(&mut self) -> [PortId; 2] {
        // adds two new associated ports, related to each other, and exposed to the proto component
        let [o, i] = [self.id_manager.new_port_id(), self.id_manager.new_port_id()];
        self.proto_component_ports.insert(o);
        self.proto_component_ports.insert(i);
        // {polarity, peer, route} known. {} unknown.
        self.port_info.polarities.insert(o, Putter);
        self.port_info.polarities.insert(i, Getter);
        self.port_info.peers.insert(o, i);
        self.port_info.peers.insert(i, o);
        let route = Route::LocalComponent(LocalComponentId::Proto(self.proto_component_id));
        self.port_info.routes.insert(o, route);
        self.port_info.routes.insert(i, route);
        log!(
            self.logger,
            "Component {:?} port pair (out->in) {:?} -> {:?}",
            self.proto_component_id,
            o,
            i
        );
        [o, i]
    }
}
impl SyncProtoContext<'_> {
    pub fn is_firing(&mut self, port: PortId) -> Option<bool> {
        let var = self.port_info.firing_var_for(port);
        self.predicate.query(var)
    }
    pub fn read_msg(&mut self, port: PortId) -> Option<&Payload> {
        self.inbox.get(&port)
    }
}

impl Connector {
    pub fn gotten(&mut self, port: PortId) -> Result<&Payload, GottenError> {
        use GottenError::*;
        match &mut self.phased {
            ConnectorPhased::Setup { .. } => Err(NoPreviousRound),
            ConnectorPhased::Communication { round_result, .. } => match round_result {
                Err(_) => Err(PreviousSyncFailed),
                Ok(None) => Err(NoPreviousRound),
                Ok(Some((_index, gotten))) => gotten.get(&port).ok_or(PortDidntGet),
            },
        }
    }
    pub fn put(&mut self, port: PortId, payload: Payload) -> Result<(), PortOpError> {
        use PortOpError::*;
        if !self.native_ports.contains(&port) {
            return Err(PortUnavailable);
        }
        if Putter != *self.port_info.polarities.get(&port).unwrap() {
            return Err(WrongPolarity);
        }
        match &mut self.phased {
            ConnectorPhased::Setup { .. } => Err(NotConnected),
            ConnectorPhased::Communication { native_batches, .. } => {
                let batch = native_batches.last_mut().unwrap();
                if batch.to_put.contains_key(&port) {
                    return Err(MultipleOpsOnPort);
                }
                batch.to_put.insert(port, payload);
                Ok(())
            }
        }
    }
    pub fn next_batch(&mut self) -> Result<usize, NextBatchError> {
        // returns index of new batch
        use NextBatchError::*;
        match &mut self.phased {
            ConnectorPhased::Setup { .. } => Err(NotConnected),
            ConnectorPhased::Communication { native_batches, .. } => {
                native_batches.push(Default::default());
                Ok(native_batches.len() - 1)
            }
        }
    }
    pub fn get(&mut self, port: PortId) -> Result<(), PortOpError> {
        use PortOpError::*;
        if !self.native_ports.contains(&port) {
            return Err(PortUnavailable);
        }
        if Getter != *self.port_info.polarities.get(&port).unwrap() {
            return Err(WrongPolarity);
        }
        match &mut self.phased {
            ConnectorPhased::Setup { .. } => Err(NotConnected),
            ConnectorPhased::Communication { native_batches, .. } => {
                let batch = native_batches.last_mut().unwrap();
                if !batch.to_get.insert(port) {
                    return Err(MultipleOpsOnPort);
                }
                Ok(())
            }
        }
    }
    pub fn sync(&mut self, timeout: Duration) -> Result<usize, SyncError> {
        use SyncError::*;
        match &mut self.phased {
            ConnectorPhased::Setup { .. } => Err(NotConnected),
            ConnectorPhased::Communication {
                round_index,
                neighborhood,
                native_batches,
                endpoint_manager,
                round_result,
                ..
            } => {
                let deadline = Instant::now() + timeout;
                let logger: &mut dyn Logger = &mut *self.logger;
                // 1. run all proto components to Nonsync blockers
                log!(
                    logger,
                    "~~~ SYNC called with timeout {:?}; starting round {}",
                    &timeout,
                    round_index
                );
                let mut branching_proto_components =
                    HashMap::<ProtoComponentId, BranchingProtoComponent>::default();
                let mut unrun_components: Vec<(ProtoComponentId, ProtoComponent)> =
                    self.proto_components.iter().map(|(&k, v)| (k, v.clone())).collect();
                log!(logger, "Nonsync running {} proto components...", unrun_components.len());
                while let Some((proto_component_id, mut component)) = unrun_components.pop() {
                    // TODO coalesce fields
                    log!(
                        logger,
                        "Nonsync running proto component with ID {:?}. {} to go after this",
                        proto_component_id,
                        unrun_components.len()
                    );
                    let mut ctx = NonsyncProtoContext {
                        logger: &mut *logger,
                        port_info: &mut self.port_info,
                        id_manager: &mut self.id_manager,
                        proto_component_id,
                        unrun_components: &mut unrun_components,
                        proto_component_ports: &mut self
                            .proto_components
                            .get_mut(&proto_component_id)
                            .unwrap()
                            .ports,
                    };
                    let blocker = component.state.nonsync_run(&mut ctx, &self.proto_description);
                    log!(
                        logger,
                        "proto component {:?} ran to nonsync blocker {:?}",
                        proto_component_id,
                        &blocker
                    );
                    use NonsyncBlocker as B;
                    match blocker {
                        B::ComponentExit => drop(component),
                        B::Inconsistent => {
                            return Err(InconsistentProtoComponent(proto_component_id))
                        }
                        B::SyncBlockStart => {
                            branching_proto_components.insert(
                                proto_component_id,
                                BranchingProtoComponent::initial(component),
                            );
                        }
                    }
                }
                log!(
                    logger,
                    "All {} proto components are now done with Nonsync phase",
                    branching_proto_components.len(),
                );

                // NOTE: all msgs in outbox are of form (Getter, Payload)
                let mut payloads_to_get: Vec<(PortId, SendPayloadMsg)> = vec![];

                // create the solution storage
                let mut solution_storage = {
                    let n = std::iter::once(Route::LocalComponent(LocalComponentId::Native));
                    let c = self
                        .proto_components
                        .keys()
                        .map(|&id| Route::LocalComponent(LocalComponentId::Proto(id)));
                    let e = neighborhood.children.iter().map(|&index| Route::Endpoint { index });
                    SolutionStorage::new(n.chain(c).chain(e))
                };
                log!(logger, "Solution storage initialized");

                // 2. kick off the native
                log!(
                    logger,
                    "Translating {} native batches into branches...",
                    native_batches.len()
                );
                let mut branching_native = BranchingNative { branches: Default::default() };
                for (index, NativeBatch { to_get, to_put }) in native_batches.drain(..).enumerate()
                {
                    let predicate = {
                        let mut predicate = Predicate::default();
                        // assign trues
                        for &port in to_get.iter().chain(to_put.keys()) {
                            let var = self.port_info.firing_var_for(port);
                            predicate.assigned.insert(var, true);
                        }
                        // assign falses
                        for &port in self.native_ports.iter() {
                            let var = self.port_info.firing_var_for(port);
                            predicate.assigned.entry(var).or_insert(false);
                        }
                        predicate
                    };
                    log!(logger, "Native branch {} has pred {:?}", index, &predicate);

                    // put all messages
                    for (putter, payload) in to_put {
                        let msg = SendPayloadMsg { predicate: predicate.clone(), payload };
                        log!(logger, "Native branch {} sending msg {:?}", index, &msg);
                        // rely on invariant: sync batches respect port polarity
                        let getter = *self.port_info.peers.get(&putter).unwrap();
                        payloads_to_get.push((getter, msg));
                    }
                    if to_get.is_empty() {
                        log!(
                            logger,
                            "Native submitting solution for batch {} with {:?}",
                            index,
                            &predicate
                        );
                        solution_storage.submit_and_digest_subtree_solution(
                            logger,
                            Route::LocalComponent(LocalComponentId::Native),
                            predicate.clone(),
                        );
                    }
                    let branch = NativeBranch { index, gotten: Default::default(), to_get };
                    if let Some(existing) = branching_native.branches.insert(predicate, branch) {
                        // TODO
                        return Err(IndistinguishableBatches([index, existing.index]));
                    }
                }
                log!(logger, "Done translating native batches into branches");
                native_batches.push(Default::default());

                // run all proto components to their sync blocker
                log!(
                    logger,
                    "Running all {} proto components to their sync blocker...",
                    branching_proto_components.len()
                );
                for (&proto_component_id, proto_component) in branching_proto_components.iter_mut()
                {
                    let Self { port_info, proto_description, .. } = self;
                    let BranchingProtoComponent { ports, branches } = proto_component;
                    let mut swap = HashMap::default();
                    let mut blocked = HashMap::default();
                    // drain from branches --> blocked
                    let cd = CyclicDrainer::new(branches, &mut swap, &mut blocked);
                    BranchingProtoComponent::drain_branches_to_blocked(
                        cd,
                        logger,
                        port_info,
                        proto_description,
                        &mut solution_storage,
                        |putter, m| {
                            let getter = *port_info.peers.get(&putter).unwrap();
                            payloads_to_get.push((getter, m));
                        },
                        proto_component_id,
                        ports,
                    );
                    // swap the blocked branches back
                    std::mem::swap(&mut blocked, branches);
                }
                log!(logger, "All proto components are blocked");

                log!(logger, "Entering decision loop...");
                endpoint_manager.undelay_all();
                let decision = 'undecided: loop {
                    // drain payloads_to_get, sending them through endpoints / feeding them to components
                    while let Some((getter, send_payload_msg)) = payloads_to_get.pop() {
                        assert!(self.port_info.polarities.get(&getter).copied() == Some(Getter));
                        match self.port_info.routes.get(&getter).unwrap() {
                            Route::Endpoint { index } => {
                                let msg = Msg::CommMsg(CommMsg {
                                    round_index: *round_index,
                                    contents: CommMsgContents::SendPayload(send_payload_msg),
                                });
                                endpoint_manager.send_to(*index, &msg).unwrap();
                            }
                            Route::LocalComponent(LocalComponentId::Native) => branching_native
                                .feed_msg(
                                    logger,
                                    &self.port_info,
                                    &mut solution_storage,
                                    getter,
                                    send_payload_msg,
                                ),
                            Route::LocalComponent(LocalComponentId::Proto(proto_component_id)) => {
                                if let Some(branching_component) =
                                    branching_proto_components.get_mut(proto_component_id)
                                {
                                    let proto_component_id = *proto_component_id;
                                    let Self { port_info, proto_description, .. } = self;
                                    branching_component.feed_msg(
                                        logger,
                                        port_info,
                                        proto_description,
                                        &mut solution_storage,
                                        proto_component_id,
                                        |putter, m| {
                                            let getter = *port_info.peers.get(&putter).unwrap();
                                            payloads_to_get.push((getter, m));
                                        },
                                        getter,
                                        send_payload_msg,
                                    )
                                }
                            }
                        }
                    }

                    // check if we have a solution yet
                    log!(logger, "Check if we have any local decisions...");
                    for solution in solution_storage.iter_new_local_make_old() {
                        log!(logger, "New local decision with solution {:?}...", &solution);
                        match neighborhood.parent {
                            Some(parent) => {
                                log!(logger, "Forwarding to my parent {:?}", parent);
                                let suggestion = Decision::Success(solution);
                                let msg = Msg::CommMsg(CommMsg {
                                    round_index: *round_index,
                                    contents: CommMsgContents::Suggest { suggestion },
                                });
                                endpoint_manager.send_to(parent, &msg).unwrap();
                            }
                            None => {
                                log!(logger, "No parent. Deciding on solution {:?}", &solution);
                                break 'undecided Decision::Success(solution);
                            }
                        }
                    }

                    // stuck! make progress by receiving a msg
                    // try recv messages arriving through endpoints
                    log!(logger, "No decision yet. Let's recv an endpoint msg...");
                    {
                        let (endpoint_index, msg) =
                            endpoint_manager.try_recv_any(deadline).unwrap();
                        log!(logger, "Received from endpoint {} msg {:?}", endpoint_index, &msg);
                        let comm_msg_contents = match msg {
                            Msg::SetupMsg(..) => {
                                log!(logger, "Discarding setup message; that phase is over");
                                continue 'undecided;
                            }
                            Msg::CommMsg(comm_msg) => match comm_msg.round_index.cmp(round_index) {
                                Ordering::Equal => comm_msg.contents,
                                Ordering::Less => {
                                    log!(
                                        logger,
                                        "We are in round {}, but msg is for round {}. Discard",
                                        comm_msg.round_index,
                                        round_index,
                                    );
                                    drop(comm_msg);
                                    continue 'undecided;
                                }
                                Ordering::Greater => {
                                    log!(
                                        logger,
                                        "We are in round {}, but msg is for round {}. Buffer",
                                        comm_msg.round_index,
                                        round_index,
                                    );
                                    endpoint_manager
                                        .delayed_messages
                                        .push((endpoint_index, Msg::CommMsg(comm_msg)));
                                    continue 'undecided;
                                }
                            },
                        };
                        match comm_msg_contents {
                            CommMsgContents::SendPayload(send_payload_msg) => {
                                let getter = endpoint_manager.endpoint_exts[endpoint_index]
                                    .getter_for_incoming;
                                assert!(self.port_info.polarities.get(&getter) == Some(&Getter));
                                log!(
                                    logger,
                                    "Msg routed to getter port {:?}. Buffer for recv loop",
                                    getter,
                                );
                                payloads_to_get.push((getter, send_payload_msg));
                            }
                            CommMsgContents::Suggest { suggestion } => {
                                // only accept this control msg through a child endpoint
                                if neighborhood.children.binary_search(&endpoint_index).is_ok() {
                                    match suggestion {
                                        Decision::Success(predicate) => {
                                            // child solution contributes to local solution
                                            log!(
                                                logger,
                                                "Child provided solution {:?}",
                                                &predicate
                                            );
                                            let route = Route::Endpoint { index: endpoint_index };
                                            solution_storage.submit_and_digest_subtree_solution(
                                                logger, route, predicate,
                                            );
                                        }
                                        Decision::Failure => match neighborhood.parent {
                                            None => {
                                                log!(
                                                    logger,
                                                    "As sink, I decide on my child's failure"
                                                );
                                                // I am the sink. Decide on failed
                                                break 'undecided Decision::Failure;
                                            }
                                            Some(parent) => {
                                                log!(logger, "Forwarding failure through my parent endpoint {:?}", parent);
                                                // I've got a parent. Forward the failure suggestion.
                                                let msg = Msg::CommMsg(CommMsg {
                                                    round_index: *round_index,
                                                    contents: CommMsgContents::Suggest {
                                                        suggestion,
                                                    },
                                                });
                                                endpoint_manager.send_to(parent, &msg).unwrap();
                                            }
                                        },
                                    }
                                } else {
                                    log!(logger, "Discarding suggestion {:?} from non-child endpoint idx {:?}", &suggestion, endpoint_index);
                                }
                            }
                            CommMsgContents::Announce { decision } => {
                                if Some(endpoint_index) == neighborhood.parent {
                                    // adopt this decision
                                    break 'undecided decision;
                                } else {
                                    log!(logger, "Discarding announcement {:?} from non-parent endpoint idx {:?}", &decision, endpoint_index);
                                }
                            }
                        }
                    }
                    log!(logger, "Endpoint msg recv done");
                };
                log!(logger, "Committing to decision {:?}!", &decision);

                // propagate the decision to children
                let msg = Msg::CommMsg(CommMsg {
                    round_index: *round_index,
                    contents: CommMsgContents::Announce { decision: decision.clone() },
                });
                log!(
                    logger,
                    "Announcing decision {:?} through child endpoints {:?}",
                    &msg,
                    &neighborhood.children
                );
                for &child in neighborhood.children.iter() {
                    endpoint_manager.send_to(child, &msg).unwrap();
                }

                *round_result = match decision {
                    Decision::Failure => Err(DistributedTimeout),
                    Decision::Success(predicate) => {
                        // commit changes to component states
                        self.proto_components.clear();
                        self.proto_components.extend(
                            branching_proto_components
                                .into_iter()
                                .map(|(id, bpc)| (id, bpc.collapse_with(&predicate))),
                        );
                        Ok(Some(branching_native.collapse_with(&predicate)))
                    }
                };
                log!(logger, "Updated round_result to {:?}", round_result);

                let returning = round_result
                    .as_ref()
                    .map(|option| option.as_ref().unwrap().0)
                    .map_err(|sync_error| sync_error.clone());
                log!(logger, "Returning {:?}", &returning);
                returning
            }
        }
    }
}
impl BranchingNative {
    fn feed_msg(
        &mut self,
        logger: &mut dyn Logger,
        port_info: &PortInfo,
        solution_storage: &mut SolutionStorage,
        getter: PortId,
        send_payload_msg: SendPayloadMsg,
    ) {
        log!(logger, "feeding native getter {:?} {:?}", getter, &send_payload_msg);
        assert!(port_info.polarities.get(&getter).copied() == Some(Getter));
        let mut draining = HashMap::default();
        let finished = &mut self.branches;
        std::mem::swap(&mut draining, finished);
        for (predicate, mut branch) in draining.drain() {
            log!(logger, "visiting native branch {:?} with {:?}", &branch, &predicate);
            // check if this branch expects to receive it
            let var = port_info.firing_var_for(getter);
            let mut feed_branch = |branch: &mut NativeBranch, predicate: &Predicate| {
                let was = branch.gotten.insert(getter, send_payload_msg.payload.clone());
                assert!(was.is_none());
                branch.to_get.remove(&getter);
                if branch.to_get.is_empty() {
                    let route = Route::LocalComponent(LocalComponentId::Native);
                    solution_storage.submit_and_digest_subtree_solution(
                        logger,
                        route,
                        predicate.clone(),
                    );
                }
            };
            if predicate.query(var) != Some(true) {
                // optimization. Don't bother trying this branch
                log!(
                    logger,
                    "skipping branch with {:?} that doesn't want the message (fastpath)",
                    &predicate
                );
                finished.insert(predicate, branch);
                continue;
            }
            use CommonSatResult as Csr;
            match predicate.common_satisfier(&send_payload_msg.predicate) {
                Csr::Nonexistant => {
                    // this branch does not receive the message
                    log!(
                        logger,
                        "skipping branch with {:?} that doesn't want the message (slowpath)",
                        &predicate
                    );
                    finished.insert(predicate, branch);
                }
                Csr::Equivalent | Csr::FormerNotLatter => {
                    // retain the existing predicate, but add this payload
                    feed_branch(&mut branch, &predicate);
                    log!(logger, "branch pred covers it! Accept the msg");
                    finished.insert(predicate, branch);
                }
                Csr::LatterNotFormer => {
                    // fork branch, give fork the message and payload predicate. original branch untouched
                    let mut branch2 = branch.clone();
                    let predicate2 = send_payload_msg.predicate.clone();
                    feed_branch(&mut branch2, &predicate2);
                    log!(
                        logger,
                        "payload pred {:?} covers branch pred {:?}",
                        &predicate2,
                        &predicate
                    );
                    finished.insert(predicate, branch);
                    finished.insert(predicate2, branch2);
                }
                Csr::New(predicate2) => {
                    // fork branch, give fork the message and the new predicate. original branch untouched
                    let mut branch2 = branch.clone();
                    feed_branch(&mut branch2, &predicate2);
                    log!(
                        logger,
                        "new subsuming pred created {:?}. forking and feeding",
                        &predicate2
                    );
                    finished.insert(predicate, branch);
                    finished.insert(predicate2, branch2);
                }
            }
        }
    }
    fn collapse_with(self, solution_predicate: &Predicate) -> (usize, HashMap<PortId, Payload>) {
        for (branch_predicate, branch) in self.branches {
            if branch_predicate.satisfies(solution_predicate) {
                let NativeBranch { index, gotten, .. } = branch;
                return (index, gotten);
            }
        }
        panic!("Native had no branches matching pred {:?}", solution_predicate);
    }
}

impl<'a, K: Eq + Hash + 'static, V: 'static> CyclicDrainer<'a, K, V> {
    fn new(
        input: &'a mut HashMap<K, V>,
        swap: &'a mut HashMap<K, V>,
        output: &'a mut HashMap<K, V>,
    ) -> Self {
        Self { input, inner: CyclicDrainInner { swap, output } }
    }
    fn cylic_drain(self, mut func: impl FnMut(K, V, CyclicDrainInner<'_, K, V>)) {
        let Self { input, inner: CyclicDrainInner { swap, output } } = self;
        // assert!(swap.is_empty());
        while !input.is_empty() {
            for (k, v) in input.drain() {
                func(k, v, CyclicDrainInner { swap, output })
            }
        }
    }
}
impl<'a, K: Eq + Hash, V> CyclicDrainInner<'a, K, V> {
    fn add_input(&mut self, k: K, v: V) {
        self.swap.insert(k, v);
    }
    fn add_output(&mut self, k: K, v: V) {
        self.output.insert(k, v);
    }
}

impl ProtoComponentBranch {
    fn feed_msg(&mut self, getter: PortId, payload: Payload) {
        let was = self.inbox.insert(getter, payload);
        assert!(was.is_none())
    }
}
impl BranchingProtoComponent {
    fn drain_branches_to_blocked(
        cd: CyclicDrainer<Predicate, ProtoComponentBranch>,
        //
        logger: &mut dyn Logger,
        port_info: &PortInfo,
        proto_description: &ProtocolDescription,
        solution_storage: &mut SolutionStorage,
        mut outbox_unqueue: impl FnMut(PortId, SendPayloadMsg),
        proto_component_id: ProtoComponentId,
        ports: &HashSet<PortId>,
    ) {
        cd.cylic_drain(|mut predicate, mut branch, mut drainer| {
            let mut ctx = SyncProtoContext {
                    logger,
                    predicate: &predicate,
                    port_info,
                    proto_component_id,
                    inbox: &branch.inbox,
                };
                let blocker = branch.state.sync_run(&mut ctx, proto_description);
                log!(
                    logger,
                    "Proto component with id {:?} branch with pred {:?} hit blocker {:?}",
                    proto_component_id,
                    &predicate,
                    &blocker,
                );
                use SyncBlocker as B;
                match blocker {
                    B::Inconsistent => {
                        // branch is inconsistent. throw it away
                        drop((predicate, branch));
                    }
                    B::SyncBlockEnd => {
                        // make concrete all variables
                        for &port in ports.iter() {
                            let var = port_info.firing_var_for(port);
                            predicate.assigned.entry(var).or_insert(false);
                        }
                        // submit solution for this component
                        solution_storage.submit_and_digest_subtree_solution(
                            logger,
                            Route::LocalComponent(LocalComponentId::Proto(proto_component_id)),
                            predicate.clone(),
                        );
                        // move to "blocked"
                        drainer.add_output(predicate, branch);
                    }
                    B::CouldntReadMsg(port) => {
                        // move to "blocked"
                        assert!(!branch.inbox.contains_key(&port));
                        drainer.add_output(predicate, branch);
                    }
                    B::CouldntCheckFiring(port) => {
                        // sanity check
                        let var = port_info.firing_var_for(port);
                        assert!(predicate.query(var).is_none());
                        // keep forks in "unblocked"
                        drainer.add_input(predicate.clone().inserted(var, false), branch.clone());
                        drainer.add_input(predicate.inserted(var, true), branch);
                    }
                    B::PutMsg(putter, payload) => {
                        // sanity check
                        assert_eq!(Some(&Putter), port_info.polarities.get(&putter));
                        // overwrite assignment
                        let var = port_info.firing_var_for(putter);

                        let was = predicate.assigned.insert(var, true);
                        if was == Some(false) {
                            log!(logger, "Proto component {:?} tried to PUT on port {:?} when pred said var {:?}==Some(false). inconsistent!", proto_component_id, putter, var);
                            // discard forever
                            drop((predicate, branch));
                        } else {
                            // keep in "unblocked"
                            log!(logger, "Proto component {:?} putting payload {:?} on port {:?} (using var {:?})", proto_component_id, &payload, putter, var);
                            outbox_unqueue(
                                putter,
                                SendPayloadMsg { predicate: predicate.clone(), payload },
                            );
                            drainer.add_input(predicate, branch);
                        }
                    }
                }
        });
    }
    fn feed_msg(
        &mut self,
        logger: &mut dyn Logger,
        port_info: &PortInfo,
        proto_description: &ProtocolDescription,
        solution_storage: &mut SolutionStorage,
        proto_component_id: ProtoComponentId,
        outbox_unqueue: impl FnMut(PortId, SendPayloadMsg),
        getter: PortId,
        send_payload_msg: SendPayloadMsg,
    ) {
        let BranchingProtoComponent { branches, ports } = self;
        let mut unblocked = HashMap::default();
        let mut blocked = HashMap::default();
        // partition drain from branches -> {unblocked, blocked}
        for (predicate, mut branch) in branches.drain() {
            use CommonSatResult as Csr;
            match predicate.common_satisfier(&send_payload_msg.predicate) {
                Csr::Nonexistant => {
                    // this branch does not receive the message
                    blocked.insert(predicate, branch);
                }
                Csr::Equivalent | Csr::FormerNotLatter => {
                    // retain the existing predicate, but add this payload
                    branch.feed_msg(getter, send_payload_msg.payload.clone());
                    unblocked.insert(predicate, branch);
                }
                Csr::LatterNotFormer => {
                    // fork branch, give fork the message and payload predicate. original branch untouched
                    let mut branch2 = branch.clone();
                    let predicate2 = send_payload_msg.predicate.clone();
                    branch2.feed_msg(getter, send_payload_msg.payload.clone());
                    blocked.insert(predicate, branch);
                    unblocked.insert(predicate2, branch2);
                }
                Csr::New(predicate2) => {
                    // fork branch, give fork the message and the new predicate. original branch untouched
                    let mut branch2 = branch.clone();
                    branch2.feed_msg(getter, send_payload_msg.payload.clone());
                    blocked.insert(predicate, branch);
                    unblocked.insert(predicate2, branch2);
                }
            }
        }
        // drain from unblocked --> blocked
        let mut swap = HashMap::default();
        let cd = CyclicDrainer::new(&mut unblocked, &mut swap, &mut blocked);
        BranchingProtoComponent::drain_branches_to_blocked(
            cd,
            logger,
            port_info,
            proto_description,
            solution_storage,
            outbox_unqueue,
            proto_component_id,
            ports,
        );
        // swap the blocked branches back
        std::mem::swap(&mut blocked, branches);
    }
    fn collapse_with(self, solution_predicate: &Predicate) -> ProtoComponent {
        let BranchingProtoComponent { ports, branches } = self;
        for (branch_predicate, branch) in branches {
            if branch_predicate.satisfies(solution_predicate) {
                let ProtoComponentBranch { state, .. } = branch;
                return ProtoComponent { state, ports };
            }
        }
        panic!("ProtoComponent had no branches matching pred {:?}", solution_predicate);
    }
    fn initial(ProtoComponent { state, ports }: ProtoComponent) -> Self {
        let branch = ProtoComponentBranch { inbox: Default::default(), state };
        Self { ports, branches: hashmap! { Predicate::default() => branch  } }
    }
}
impl SolutionStorage {
    fn new(routes: impl Iterator<Item = Route>) -> Self {
        let mut subtree_id_to_index: HashMap<Route, usize> = Default::default();
        let mut subtree_solutions = vec![];
        for key in routes {
            subtree_id_to_index.insert(key, subtree_solutions.len());
            subtree_solutions.push(Default::default())
        }
        Self {
            subtree_solutions,
            subtree_id_to_index,
            old_local: Default::default(),
            new_local: Default::default(),
        }
    }
    fn is_clear(&self) -> bool {
        self.subtree_id_to_index.is_empty()
            && self.subtree_solutions.is_empty()
            && self.old_local.is_empty()
            && self.new_local.is_empty()
    }
    fn clear(&mut self) {
        self.subtree_id_to_index.clear();
        self.subtree_solutions.clear();
        self.old_local.clear();
        self.new_local.clear();
    }
    pub(crate) fn reset(&mut self, subtree_ids: impl Iterator<Item = Route>) {
        self.subtree_id_to_index.clear();
        self.subtree_solutions.clear();
        self.old_local.clear();
        self.new_local.clear();
        for key in subtree_ids {
            self.subtree_id_to_index.insert(key, self.subtree_solutions.len());
            self.subtree_solutions.push(Default::default())
        }
    }

    pub(crate) fn peek_new_locals(&self) -> impl Iterator<Item = &Predicate> + '_ {
        self.new_local.iter()
    }

    pub(crate) fn iter_new_local_make_old(&mut self) -> impl Iterator<Item = Predicate> + '_ {
        let Self { old_local, new_local, .. } = self;
        new_local.drain().map(move |local| {
            old_local.insert(local.clone());
            local
        })
    }

    pub(crate) fn submit_and_digest_subtree_solution(
        &mut self,
        logger: &mut dyn Logger,
        subtree_id: Route,
        predicate: Predicate,
    ) {
        log!(logger, "NEW COMPONENT SOLUTION {:?} {:?}", subtree_id, &predicate);
        let index = self.subtree_id_to_index[&subtree_id];
        let left = 0..index;
        let right = (index + 1)..self.subtree_solutions.len();

        let Self { subtree_solutions, new_local, old_local, .. } = self;
        let was_new = subtree_solutions[index].insert(predicate.clone());
        if was_new {
            let set_visitor = left.chain(right).map(|index| &subtree_solutions[index]);
            Self::elaborate_into_new_local_rec(
                logger,
                predicate,
                set_visitor,
                old_local,
                new_local,
            );
        }
    }

    fn elaborate_into_new_local_rec<'a, 'b>(
        logger: &mut dyn Logger,
        partial: Predicate,
        mut set_visitor: impl Iterator<Item = &'b HashSet<Predicate>> + Clone,
        old_local: &'b HashSet<Predicate>,
        new_local: &'a mut HashSet<Predicate>,
    ) {
        if let Some(set) = set_visitor.next() {
            // incomplete solution. keep traversing
            for pred in set.iter() {
                if let Some(elaborated) = pred.union_with(&partial) {
                    Self::elaborate_into_new_local_rec(
                        logger,
                        elaborated,
                        set_visitor.clone(),
                        old_local,
                        new_local,
                    )
                }
            }
        } else {
            // recursive stop condition. `partial` is a local subtree solution
            if !old_local.contains(&partial) {
                // ... and it hasn't been found before
                log!(logger, "storing NEW LOCAL SOLUTION {:?}", &partial);
                new_local.insert(partial);
            }
        }
    }
}
