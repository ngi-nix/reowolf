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
struct BranchingProtoComponent {
    ports: HashSet<PortId>,
    branches: HashMap<Predicate, ProtoComponentBranch>,
}
#[derive(Clone)]
struct ProtoComponentBranch {
    inbox: HashMap<PortId, Payload>,
    state: ComponentState,
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
                    let e = (0..endpoint_manager.endpoint_exts.len())
                        .map(|index| Route::Endpoint { index });
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
                        log!(logger, "Native submitting trivial solution for index {}", index);
                        solution_storage.submit_and_digest_subtree_solution(
                            logger,
                            Route::LocalComponent(LocalComponentId::Native),
                            Predicate::default(),
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
                for (proto_component_id, proto_component) in branching_proto_components.iter_mut() {
                    // run this component to sync blocker in-place
                    log!(
                        logger,
                        "Running proto component with id {:?} to blocker...",
                        proto_component_id
                    );
                    let blocked = &mut proto_component.branches;
                    let [unblocked_from, unblocked_to] = [
                        &mut HashMap::<Predicate, ProtoComponentBranch>::default(),
                        &mut Default::default(),
                    ];
                    // DRAIN-AND-POPULATE PATTERN: DRAINING unblocked into blocked while POPULATING unblocked
                    std::mem::swap(unblocked_from, blocked);
                    while !unblocked_from.is_empty() {
                        for (mut predicate, mut branch) in unblocked_from.drain() {
                            let mut ctx = SyncProtoContext {
                                logger,
                                predicate: &predicate,
                                port_info: &self.port_info,
                                proto_component_id: *proto_component_id,
                                inbox: &branch.inbox,
                            };
                            let blocker = branch.state.sync_run(&mut ctx, &self.proto_description);
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
                                    for &port in proto_component.ports.iter() {
                                        let var = self.port_info.firing_var_for(port);
                                        predicate.assigned.entry(var).or_insert(false);
                                    }
                                    // submit solution for this component
                                    solution_storage.submit_and_digest_subtree_solution(
                                        logger,
                                        Route::LocalComponent(LocalComponentId::Proto(
                                            *proto_component_id,
                                        )),
                                        predicate.clone(),
                                    );
                                    // move to "blocked"
                                    blocked.insert(predicate, branch);
                                }
                                B::CouldntReadMsg(port) => {
                                    // move to "blocked"
                                    let var = self.port_info.firing_var_for(port);
                                    assert!(predicate.query(var).is_none());
                                    assert!(!branch.inbox.contains_key(&port));
                                    blocked.insert(predicate, branch);
                                }
                                B::CouldntCheckFiring(port) => {
                                    // sanity check
                                    let var = self.port_info.firing_var_for(port);
                                    assert!(predicate.query(var).is_none());
                                    // keep forks in "unblocked"
                                    unblocked_to.insert(
                                        predicate.clone().inserted(var, false),
                                        branch.clone(),
                                    );
                                    unblocked_to.insert(predicate.inserted(var, true), branch);
                                }
                                B::PutMsg(putter, payload) => {
                                    // sanity check
                                    assert_eq!(
                                        Some(&Putter),
                                        self.port_info.polarities.get(&putter)
                                    );
                                    // overwrite assignment
                                    let var = self.port_info.firing_var_for(putter);

                                    let was = predicate.assigned.insert(var, true);
                                    if was == Some(false) {
                                        log!(logger, "Proto component {:?} tried to PUT on port {:?} when pred said var {:?}==Some(false). inconsistent!", proto_component_id, putter, var);
                                        // discard forever
                                        drop((predicate, branch));
                                    } else {
                                        // keep in "unblocked"
                                        let getter = *self.port_info.peers.get(&putter).unwrap();
                                        log!(logger, "Proto component {:?} putting payload {:?} on port {:?} (using var {:?})", proto_component_id, &payload, putter, var);
                                        payloads_to_get.push((
                                            getter,
                                            SendPayloadMsg {
                                                predicate: predicate.clone(),
                                                payload,
                                            },
                                        ));
                                        unblocked_to.insert(predicate, branch);
                                    }
                                }
                            }
                        }
                        std::mem::swap(unblocked_from, unblocked_to);
                    }
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
                                    branching_proto_components.get_mut(&proto_component_id)
                                {
                                    branching_component.feed_msg(
                                        logger,
                                        &self.port_info,
                                        &mut solution_storage,
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
        assert!(port_info.polarities.get(&getter).copied() == Some(Getter));
        println!("BEFORE {:#?}", &self.branches);
        let mut draining = HashMap::default();
        let finished = &mut self.branches;
        std::mem::swap(&mut draining, finished);
        for (predicate, mut branch) in draining.drain() {
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
                finished.insert(predicate, branch);
                continue;
            }
            use CommonSatResult as Csr;
            match predicate.common_satisfier(&send_payload_msg.predicate) {
                Csr::Equivalent | Csr::FormerNotLatter => {
                    // retain the existing predicate, but add this payload
                    feed_branch(&mut branch, &predicate);
                    finished.insert(predicate, branch);
                }
                Csr::Nonexistant => {
                    // this branch does not receive the message
                    finished.insert(predicate, branch);
                }
                Csr::LatterNotFormer => {
                    // fork branch, give fork the message and payload predicate
                    let mut branch2 = branch.clone();
                    // original branch untouched
                    finished.insert(predicate, branch);
                    let predicate2 = send_payload_msg.predicate.clone();
                    feed_branch(&mut branch2, &predicate2);
                    finished.insert(predicate2, branch2);
                }
                Csr::New(new_predicate) => {
                    // fork branch, give fork the message and the new predicate
                    let mut branch2 = branch.clone();
                    // original branch untouched
                    finished.insert(predicate, branch);
                    feed_branch(&mut branch2, &new_predicate);
                    finished.insert(new_predicate, branch2);
                }
            }
        }
        println!("AFTER {:#?}", &self.branches);
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
impl BranchingProtoComponent {
    fn feed_msg(
        &mut self,
        _logger: &mut dyn Logger,
        _port_info: &PortInfo,
        _solution_storage: &mut SolutionStorage,
        _getter: PortId,
        _send_payload_msg: SendPayloadMsg,
    ) {
        todo!()
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

// impl ControllerEphemeral {
//     fn is_clear(&self) -> bool {
//         self.solution_storage.is_clear()
//             && self.poly_n.is_none()
//             && self.poly_ps.is_empty()
//             && self.mono_ps.is_empty()
//             && self.port_to_holder.is_empty()
//     }
//     fn clear(&mut self) {
//         self.solution_storage.clear();
//         self.poly_n.take();
//         self.poly_ps.clear();
//         self.port_to_holder.clear();
//     }
// }
// impl Into<PolyP> for MonoP {
//     fn into(self) -> PolyP {
//         PolyP {
//             complete: Default::default(),
//             incomplete: hashmap! {
//                 Predicate::new_trivial() =>
//                 BranchP {
//                     state: self.state,
//                     inbox: Default::default(),
//                     outbox: Default::default(),
//                     blocking_on: None,
//                 }
//             },
//             ports: self.ports,
//         }
//     }
// }

// impl From<EndpointError> for SyncError {
//     fn from(e: EndpointError) -> SyncError {
//         SyncError::EndpointError(e)
//     }
// }

// impl ProtoSyncContext<'_> {
//     fn new_component(&mut self, moved_ports: HashSet<PortId>, init_state: Self::S) {
//         todo!()
//     }
//     fn new_channel(&mut self) -> [PortId; 2] {
//         todo!()
//     }
// }

// impl PolyContext for BranchPContext<'_, '_> {
//     type D = ProtocolD;

//     fn is_firing(&mut self, port: PortId) -> Option<bool> {
//         assert!(self.ports.contains(&port));
//         let channel_id = self.m_ctx.endpoint_exts.get(port).unwrap().info.channel_id;
//         let val = self.predicate.query(channel_id);
//         log!(
//             &mut self.m_ctx.logger,
//             "!! PolyContext callback to is_firing by {:?}! returning {:?}",
//             self.m_ctx.my_subtree_id,
//             val,
//         );
//         val
//     }
//     fn read_msg(&mut self, port: PortId) -> Option<&Payload> {
//         assert!(self.ports.contains(&port));
//         let val = self.inbox.get(&port);
//         log!(
//             &mut self.m_ctx.logger,
//             "!! PolyContext callback to read_msg by {:?}! returning {:?}",
//             self.m_ctx.my_subtree_id,
//             val,
//         );
//         val
//     }
// }

//////////////

// impl Connector {
// fn end_round_with_decision(&mut self, decision: Decision) -> Result<usize, SyncError> {
//     log!(&mut self.logger, "ENDING ROUND WITH DECISION! {:?}", &decision);
//     let ret = match &decision {
//         Decision::Success(predicate) => {
//             // overwrite MonoN/P
//             self.mono_n = {
//                 let poly_n = self.ephemeral.poly_n.take().unwrap();
//                 poly_n.choose_mono(predicate).unwrap_or_else(|| {
//                     panic!(
//                         "Ending round with decision pred {:#?} but poly_n has branches {:#?}. My log is... {}",
//                         &predicate, &poly_n.branches, &self.logger
//                     );
//                 })
//             };
//             self.mono_ps.clear();
//             self.mono_ps.extend(
//                 self.ephemeral
//                     .poly_ps
//                     .drain(..)
//                     .map(|poly_p| poly_p.choose_mono(predicate).unwrap()),
//             );
//             Ok(())
//         }
//         Decision::Failure => Err(SyncError::Timeout),
//     };
//     let announcement = CommMsgContents::Announce { decision }.into_msg(self.round_index);
//     for &child_port in self.family.children_ports.iter() {
//         log!(
//             &mut self.logger,
//             "Forwarding {:?} to child with port {:?}",
//             &announcement,
//             child_port
//         );
//         self.endpoint_exts
//             .get_mut(child_port)
//             .expect("eefef")
//             .endpoint
//             .send(announcement.clone())?;
//     }
//     self.round_index += 1;
//     self.ephemeral.clear();
//     ret
// }

// // Drain self.ephemeral.solution_storage and handle the new locals. Return decision if one is found
// fn handle_locals_maybe_decide(&mut self) -> Result<bool, SyncError> {
//     if let Some(parent_port) = self.family.parent_port {
//         // I have a parent -> I'm not the leader
//         let parent_endpoint =
//             &mut self.endpoint_exts.get_mut(parent_port).expect("huu").endpoint;
//         for partial_oracle in self.ephemeral.solution_storage.iter_new_local_make_old() {
//             let msg = CommMsgContents::Elaborate { partial_oracle }.into_msg(self.round_index);
//             log!(&mut self.logger, "Sending {:?} to parent {:?}", &msg, parent_port);
//             parent_endpoint.send(msg)?;
//         }
//         Ok(false)
//     } else {
//         // I have no parent -> I'm the leader
//         assert!(self.family.parent_port.is_none());
//         let maybe_predicate = self.ephemeral.solution_storage.iter_new_local_make_old().next();
//         Ok(if let Some(predicate) = maybe_predicate {
//             let decision = Decision::Success(predicate);
//             log!(&mut self.logger, "DECIDE ON {:?} AS LEADER!", &decision);
//             self.end_round_with_decision(decision)?;
//             true
//         } else {
//             false
//         })
//     }
// }

// fn kick_off_native(
//     &mut self,
//     sync_batches: impl Iterator<Item = SyncBatch>,
// ) -> Result<PolyN, EndpointError> {
//     let MonoN { ports, .. } = self.mono_n.clone();
//     let Self { inner: ControllerInner { endpoint_exts, round_index, .. }, .. } = self;
//     let mut branches = HashMap::<_, _>::default();
//     for (sync_batch_index, SyncBatch { puts, gets }) in sync_batches.enumerate() {
//         let port_to_channel_id = |port| endpoint_exts.get(port).unwrap().info.channel_id;
//         let all_ports = ports.iter().copied();
//         let all_channel_ids = all_ports.map(port_to_channel_id);

//         let mut predicate = Predicate::new_trivial();

//         // assign TRUE for puts and gets
//         let true_ports = puts.keys().chain(gets.iter()).copied();
//         let true_channel_ids = true_ports.clone().map(port_to_channel_id);
//         predicate.batch_assign_nones(true_channel_ids, true);

//         // assign FALSE for all in interface not assigned true
//         predicate.batch_assign_nones(all_channel_ids.clone(), false);

//         if branches.contains_key(&predicate) {
//             // TODO what do I do with redundant predicates?
//             unimplemented!(
//                 "Duplicate predicate {:#?}!\nHaving multiple batches with the same
//                 predicate requires the support of oracle boolean variables",
//                 &predicate,
//             )
//         }
//         let branch = BranchN { to_get: gets, gotten: Default::default(), sync_batch_index };
//         for (port, payload) in puts {
//             log!(
//                 &mut self.logger,
//                 "... ... Initial native put msg {:?} pred {:?} batch {:?}",
//                 &payload,
//                 &predicate,
//                 sync_batch_index,
//             );
//             let msg =
//                 CommMsgContents::SendPayload { payload_predicate: predicate.clone(), payload }
//                     .into_msg(*round_index);
//             endpoint_exts.get_mut(port).unwrap().endpoint.send(msg)?;
//         }
//         log!(
//             &mut self.logger,
//             "... Initial native branch batch index={} with pred {:?}",
//             sync_batch_index,
//             &predicate
//         );
//         if branch.to_get.is_empty() {
//             self.ephemeral.solution_storage.submit_and_digest_subtree_solution(
//                 &mut self.logger,
//                 Route::PolyN,
//                 predicate.clone(),
//             );
//         }
//         branches.insert(predicate, branch);
//     }
//     Ok(PolyN { ports, branches })
// }
// pub fn sync_round(
//     &mut self,
//     deadline: Option<Instant>,
//     sync_batches: Option<impl Iterator<Item = SyncBatch>>,
// ) -> Result<(), SyncError> {
//     if let Some(e) = self.unrecoverable_error {
//         return Err(e.clone());
//     }
//     self.sync_round_inner(deadline, sync_batches).map_err(move |e| match e {
//         SyncError::Timeout => e, // this isn't unrecoverable
//         _ => {
//             // Must set unrecoverable error! and tear down our net channels
//             self.unrecoverable_error = Some(e);
//             self.ephemeral.clear();
//             self.endpoint_exts = Default::default();
//             e
//         }
//     })
// }

// // Runs a synchronous round until all the actors are in decided state OR 1+ are inconsistent.
// // If a native requires setting up, arg `sync_batches` is Some, and those are used as the sync batches.
// fn sync_round_inner(
//     &mut self,
//     mut deadline: Option<Instant>,
//     sync_batches: Option<impl Iterator<Item = SyncBatch>>,
// ) -> Result<(), SyncError> {
//     log!(&mut self.logger, "~~~~~~~~ SYNC ROUND STARTS! ROUND={} ~~~~~~~~~", self.round_index);
//     assert!(self.ephemeral.is_clear());
//     assert!(self.unrecoverable_error.is_none());

//     // 1. Run the Mono for each Mono actor (stored in `self.mono_ps`).
//     //    Some actors are dropped. some new actors are created.
//     //    Ultimately, we have 0 Mono actors and a list of unnamed sync_actors
//     self.ephemeral.mono_ps.extend(self.mono_ps.iter().cloned());
//     log!(&mut self.logger, "Got {} MonoP's to run!", self.ephemeral.mono_ps.len());
//     while let Some(mut mono_p) = self.ephemeral.mono_ps.pop() {
//         let mut m_ctx = ProtoSyncContext {
//             ports: &mut mono_p.ports,
//             mono_ps: &mut self.ephemeral.mono_ps,
//             inner: &mut self,
//         };
//         // cross boundary into crate::protocol
//         let blocker = mono_p.state.pre_sync_run(&mut m_ctx, &self.protocol_description);
//         log!(&mut self.logger, "... MonoP's pre_sync_run got blocker {:?}", &blocker);
//         match blocker {
//             NonsyncBlocker::Inconsistent => return Err(SyncError::Inconsistent),
//             NonsyncBlocker::ComponentExit => drop(mono_p),
//             NonsyncBlocker::SyncBlockStart => self.ephemeral.poly_ps.push(mono_p.into()),
//         }
//     }
//     log!(
//         &mut self.logger,
//         "Finished running all MonoPs! Have {} PolyPs waiting",
//         self.ephemeral.poly_ps.len()
//     );

//     // 3. define the mapping from port -> actor
//     //    this is needed during the event loop to determine which actor
//     //    should receive the incoming message.
//     //    TODO: store and update this mapping rather than rebuilding it each round.
//     let port_to_holder: HashMap<PortId, PolyId> = {
//         use PolyId::*;
//         let n = self.mono_n.ports.iter().map(move |&e| (e, N));
//         let p = self
//             .ephemeral
//             .poly_ps
//             .iter()
//             .enumerate()
//             .flat_map(|(index, m)| m.ports.iter().map(move |&e| (e, P { index })));
//         n.chain(p).collect()
//     };
//     log!(
//         &mut self.logger,
//         "SET OF PolyPs and MonoPs final! port lookup map is {:?}",
//         &port_to_holder
//     );

//     // 4. Create the solution storage. it tracks the solutions of "subtrees"
//     //    of the controller in the overlay tree.
//     self.ephemeral.solution_storage.reset({
//         let n = std::iter::once(Route::PolyN);
//         let m = (0..self.ephemeral.poly_ps.len()).map(|index| Route::PolyP { index });
//         let c = self.family.children_ports.iter().map(|&port| Route::ChildController { port });
//         let subtree_id_iter = n.chain(m).chain(c);
//         log!(
//             &mut self.logger,
//             "Solution Storage has subtree Ids: {:?}",
//             &subtree_id_iter.clone().collect::<Vec<_>>()
//         );
//         subtree_id_iter
//     });

//     // 5. kick off the synchronous round of the native actor if it exists

//     log!(&mut self.logger, "Kicking off native's synchronous round...");
//     self.ephemeral.poly_n = if let Some(sync_batches) = sync_batches {
//         // using if let because of nested ? operator
//         // TODO check that there are 1+ branches or NO SOLUTION
//         let poly_n = self.kick_off_native(sync_batches)?;
//         log!(
//             &mut self.logger,
//             "PolyN kicked off, and has branches with predicates... {:?}",
//             poly_n.branches.keys().collect::<Vec<_>>()
//         );
//         Some(poly_n)
//     } else {
//         log!(&mut self.logger, "NO NATIVE COMPONENT");
//         None
//     };

//     // 6. Kick off the synchronous round of each protocol actor
//     //    If just one actor becomes inconsistent now, there can be no solution!
//     //    TODO distinguish between completed and not completed poly_p's?
//     log!(&mut self.logger, "Kicking off {} PolyP's.", self.ephemeral.poly_ps.len());
//     for (index, poly_p) in self.ephemeral.poly_ps.iter_mut().enumerate() {
//         let my_subtree_id = Route::PolyP { index };
//         let m_ctx = PolyPContext {
//             my_subtree_id,
//             inner: &mut self,
//             solution_storage: &mut self.ephemeral.solution_storage,
//         };
//         use SyncRunResult as Srr;
//         let blocker = poly_p.poly_run(m_ctx, &self.protocol_description)?;
//         log!(&mut self.logger, "... PolyP's poly_run got blocker {:?}", &blocker);
//         match blocker {
//             Srr::NoBranches => return Err(SyncError::Inconsistent),
//             Srr::AllBranchesComplete | Srr::BlockingForRecv => (),
//         }
//     }
//     log!(&mut self.logger, "All Poly machines have been kicked off!");

//     // 7. `solution_storage` may have new solutions for this controller
//     //    handle their discovery. LEADER => announce, otherwise => send to parent
//     {
//         let peeked = self.ephemeral.solution_storage.peek_new_locals().collect::<Vec<_>>();
//         log!(
//             &mut self.logger,
//             "Got {} controller-local solutions before a single RECV: {:?}",
//             peeked.len(),
//             peeked
//         );
//     }
//     if self.handle_locals_maybe_decide()? {
//         return Ok(());
//     }

//     // 4. Receive incoming messages until the DECISION is made OR some unrecoverable error
//     log!(&mut self.logger, "`No decision yet`. Time to recv messages");
//     self.undelay_all();
//     'recv_loop: loop {
//         log!(&mut self.logger, "`POLLING` with deadline {:?}...", deadline);
//         let received = match deadline {
//             None => {
//                 // we have personally timed out. perform a "long" poll.
//                 self.recv(Instant::now() + Duration::from_secs(10))?.expect("DRIED UP")
//             }
//             Some(d) => match self.recv(d)? {
//                 // we have not yet timed out. performed a time-limited poll
//                 Some(received) => received,
//                 None => {
//                     // timed out! send a FAILURE message to the sink,
//                     // and henceforth don't time out on polling.
//                     deadline = None;
//                     match self.family.parent_port {
//                         None => {
//                             // I am the sink! announce failure and return.
//                             return self.end_round_with_decision(Decision::Failure);
//                         }
//                         Some(parent_port) => {
//                             // I am not the sink! send a failure message.
//                             let announcement = Msg::CommMsg(CommMsg {
//                                 round_index: self.round_index,
//                                 contents: CommMsgContents::Failure,
//                             });
//                             log!(
//                                 &mut self.logger,
//                                 "Forwarding {:?} to parent with port {:?}",
//                                 &announcement,
//                                 parent_port
//                             );
//                             self.endpoint_exts
//                                 .get_mut(parent_port)
//                                 .expect("ss")
//                                 .endpoint
//                                 .send(announcement.clone())?;
//                             continue; // poll some more
//                         }
//                     }
//                 }
//             },
//         };
//         log!(&mut self.logger, "::: message {:?}...", &received);
//         let current_content = match received.msg {
//             Msg::SetupMsg(s) => {
//                 // This occurs in the event the connector was malformed during connect()
//                 println!("WASNT EXPECTING {:?}", s);
//                 return Err(SyncError::UnexpectedSetupMsg);
//             }
//             Msg::CommMsg(CommMsg { round_index, .. }) if round_index < self.round_index => {
//                 // Old message! Can safely discard
//                 log!(&mut self.logger, "...and its OLD! :(");
//                 drop(received);
//                 continue 'recv_loop;
//             }
//             Msg::CommMsg(CommMsg { round_index, .. }) if round_index > self.round_index => {
//                 // Message from a next round. Keep for later!
//                 log!(&mut self.logger, "... DELAY! :(");
//                 self.delay(received);
//                 continue 'recv_loop;
//             }
//             Msg::CommMsg(CommMsg { contents, round_index }) => {
//                 log!(
//                     &mut self.logger,
//                     "... its a round-appropriate CommMsg with port {:?}",
//                     received.recipient
//                 );
//                 assert_eq!(round_index, self.round_index);
//                 contents
//             }
//         };
//         match current_content {
//             CommMsgContents::Failure => match self.family.parent_port {
//                 Some(parent_port) => {
//                     let announcement = Msg::CommMsg(CommMsg {
//                         round_index: self.round_index,
//                         contents: CommMsgContents::Failure,
//                     });
//                     log!(
//                         &mut self.logger,
//                         "Forwarding {:?} to parent with port {:?}",
//                         &announcement,
//                         parent_port
//                     );
//                     self.endpoint_exts
//                         .get_mut(parent_port)
//                         .expect("ss")
//                         .endpoint
//                         .send(announcement.clone())?;
//                 }
//                 None => return self.end_round_with_decision(Decision::Failure),
//             },
//             CommMsgContents::Elaborate { partial_oracle } => {
//                 // Child controller submitted a subtree solution.
//                 if !self.family.children_ports.contains(&received.recipient) {
//                     return Err(SyncError::ElaborateFromNonChild);
//                 }
//                 let subtree_id = Route::ChildController { port: received.recipient };
//                 log!(
//                     &mut self.logger,
//                     "Received elaboration from child for subtree {:?}: {:?}",
//                     subtree_id,
//                     &partial_oracle
//                 );
//                 self.ephemeral.solution_storage.submit_and_digest_subtree_solution(
//                     &mut self.logger,
//                     subtree_id,
//                     partial_oracle,
//                 );
//                 if self.handle_locals_maybe_decide()? {
//                     return Ok(());
//                 }
//             }
//             CommMsgContents::Announce { decision } => {
//                 if self.family.parent_port != Some(received.recipient) {
//                     return Err(SyncError::AnnounceFromNonParent);
//                 }
//                 log!(
//                     &mut self.logger,
//                     "Received ANNOUNCEMENT from from parent {:?}: {:?}",
//                     received.recipient,
//                     &decision
//                 );
//                 return self.end_round_with_decision(decision);
//             }
//             CommMsgContents::SendPayload { payload_predicate, payload } => {
//                 // check that we expect to be able to receive payloads from this sender
//                 assert_eq!(
//                     Getter,
//                     self.endpoint_exts.get(received.recipient).unwrap().info.polarity
//                 );

//                 // message for some actor. Feed it to the appropriate actor
//                 // and then give them another chance to run.
//                 let subtree_id = port_to_holder.get(&received.recipient);
//                 log!(
//                     &mut self.logger,
//                     "Received SendPayload for subtree {:?} with pred {:?} and payload {:?}",
//                     subtree_id,
//                     &payload_predicate,
//                     &payload
//                 );
//                 let channel_id =
//                     self.endpoint_exts.get(received.recipient).expect("UEHFU").info.channel_id;
//                 if payload_predicate.query(channel_id) != Some(true) {
//                     // sender didn't preserve the invariant
//                     return Err(SyncError::PayloadPremiseExcludesTheChannel(channel_id));
//                 }
//                 match subtree_id {
//                     None => {
//                         // this happens when a message is sent to a component that has exited.
//                         // It's safe to drop this message;
//                         // The sender branch will certainly not be part of the solution
//                     }
//                     Some(PolyId::N) => {
//                         // Message for NativeMachine
//                         self.ephemeral.poly_n.as_mut().unwrap().sync_recv(
//                             received.recipient,
//                             &mut self.logger,
//                             payload,
//                             payload_predicate,
//                             &mut self.ephemeral.solution_storage,
//                         );
//                         if self.handle_locals_maybe_decide()? {
//                             return Ok(());
//                         }
//                     }
//                     Some(PolyId::P { index }) => {
//                         // Message for protocol actor
//                         let poly_p = &mut self.ephemeral.poly_ps[*index];

//                         let m_ctx = PolyPContext {
//                             my_subtree_id: Route::PolyP { index: *index },
//                             inner: &mut self,
//                             solution_storage: &mut self.ephemeral.solution_storage,
//                         };
//                         use SyncRunResult as Srr;
//                         let blocker = poly_p.poly_recv_run(
//                             m_ctx,
//                             &self.protocol_description,
//                             received.recipient,
//                             payload_predicate,
//                             payload,
//                         )?;
//                         log!(
//                             &mut self.logger,
//                             "... Fed the msg to PolyP {:?} and ran it to blocker {:?}",
//                             subtree_id,
//                             blocker
//                         );
//                         match blocker {
//                             Srr::NoBranches => return Err(SyncError::Inconsistent),
//                             Srr::BlockingForRecv | Srr::AllBranchesComplete => {
//                                 {
//                                     let peeked = self
//                                         .ephemeral
//                                         .solution_storage
//                                         .peek_new_locals()
//                                         .collect::<Vec<_>>();
//                                     log!(
//                                         &mut self.logger,
//                                         "Got {} new controller-local solutions from RECV: {:?}",
//                                         peeked.len(),
//                                         peeked
//                                     );
//                                 }
//                                 if self.handle_locals_maybe_decide()? {
//                                     return Ok(());
//                                 }
//                             }
//                         }
//                     }
//                 };
//             }
//         }
//     }
// }
// }
