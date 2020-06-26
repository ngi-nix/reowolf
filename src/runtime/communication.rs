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
trait PayloadMsgSender {
    fn putter_send(
        &mut self,
        cu: &mut ConnectorUnphased,
        putter: PortId,
        msg: SendPayloadMsg,
    ) -> Result<(), SyncError>;
}
trait ReplaceBoolTrue {
    fn replace_with_true(&mut self) -> bool;
}
impl ReplaceBoolTrue for bool {
    fn replace_with_true(&mut self) -> bool {
        let was = *self;
        *self = true;
        !was
    }
}

////////////////
impl Connector {
    pub fn gotten(&mut self, port: PortId) -> Result<&Payload, GottenError> {
        use GottenError::*;
        let Self { phased, .. } = self;
        match phased {
            ConnectorPhased::Setup { .. } => Err(NoPreviousRound),
            ConnectorPhased::Communication(comm) => match &comm.round_result {
                Err(_) => Err(PreviousSyncFailed),
                Ok(None) => Err(NoPreviousRound),
                Ok(Some(round_ok)) => round_ok.gotten.get(&port).ok_or(PortDidntGet),
            },
        }
    }
    pub fn next_batch(&mut self) -> Result<usize, NextBatchError> {
        // returns index of new batch
        use NextBatchError::*;
        let Self { phased, .. } = self;
        match phased {
            ConnectorPhased::Setup { .. } => Err(NotConnected),
            ConnectorPhased::Communication(comm) => {
                comm.native_batches.push(Default::default());
                Ok(comm.native_batches.len() - 1)
            }
        }
    }
    fn port_op_access(
        &mut self,
        port: PortId,
        expect_polarity: Polarity,
    ) -> Result<&mut NativeBatch, PortOpError> {
        use PortOpError::*;
        let Self { unphased, phased } = self;
        if !unphased.native_ports.contains(&port) {
            return Err(PortUnavailable);
        }
        match unphased.port_info.polarities.get(&port) {
            Some(p) if *p == expect_polarity => {}
            Some(_) => return Err(WrongPolarity),
            None => return Err(UnknownPolarity),
        }
        match phased {
            ConnectorPhased::Setup { .. } => Err(NotConnected),
            ConnectorPhased::Communication(comm) => {
                let batch = comm.native_batches.last_mut().unwrap(); // length >= 1 is invariant
                Ok(batch)
            }
        }
    }
    pub fn put(&mut self, port: PortId, payload: Payload) -> Result<(), PortOpError> {
        use PortOpError::*;
        let batch = self.port_op_access(port, Putter)?;
        if batch.to_put.contains_key(&port) {
            Err(MultipleOpsOnPort)
        } else {
            batch.to_put.insert(port, payload);
            Ok(())
        }
    }
    pub fn get(&mut self, port: PortId) -> Result<(), PortOpError> {
        use PortOpError::*;
        let batch = self.port_op_access(port, Getter)?;
        if batch.to_get.insert(port) {
            Ok(())
        } else {
            Err(MultipleOpsOnPort)
        }
    }
    // entrypoint for caller. overwrites round result enum, and returns what happened
    pub fn sync(&mut self, timeout: Option<Duration>) -> Result<usize, SyncError> {
        let Self { unphased, phased } = self;
        match phased {
            ConnectorPhased::Setup { .. } => Err(SyncError::NotConnected),
            ConnectorPhased::Communication(comm) => {
                comm.round_result = Self::connected_sync(unphased, comm, timeout);
                match &comm.round_result {
                    Ok(None) => unreachable!(),
                    Ok(Some(ok_result)) => Ok(ok_result.batch_index),
                    Err(sync_error) => Err(sync_error.clone()),
                }
            }
        }
    }
    // private function. mutates state but returns with round
    // result ASAP (allows for convenient error return with ?)
    fn connected_sync(
        cu: &mut ConnectorUnphased,
        comm: &mut ConnectorCommunication,
        timeout: Option<Duration>,
    ) -> Result<Option<RoundOk>, SyncError> {
        use SyncError as Se;
        let deadline = timeout.map(|to| Instant::now() + to);
        log!(
            cu.logger,
            "~~~ SYNC called with timeout {:?}; starting round {}",
            &timeout,
            comm.round_index
        );

        // 1. run all proto components to Nonsync blockers
        let mut branching_proto_components =
            HashMap::<ProtoComponentId, BranchingProtoComponent>::default();
        let mut unrun_components: Vec<(ProtoComponentId, ProtoComponent)> =
            cu.proto_components.iter().map(|(&k, v)| (k, v.clone())).collect();
        log!(cu.logger, "Nonsync running {} proto components...", unrun_components.len());
        while let Some((proto_component_id, mut component)) = unrun_components.pop() {
            // TODO coalesce fields
            log!(
                cu.logger,
                "Nonsync running proto component with ID {:?}. {} to go after this",
                proto_component_id,
                unrun_components.len()
            );
            let mut ctx = NonsyncProtoContext {
                logger: &mut *cu.logger,
                port_info: &mut cu.port_info,
                id_manager: &mut cu.id_manager,
                proto_component_id,
                unrun_components: &mut unrun_components,
                proto_component_ports: &mut cu
                    .proto_components
                    .get_mut(&proto_component_id)
                    .unwrap() // unrun_components' keys originate from proto_components
                    .ports,
            };
            let blocker = component.state.nonsync_run(&mut ctx, &cu.proto_description);
            log!(
                cu.logger,
                "proto component {:?} ran to nonsync blocker {:?}",
                proto_component_id,
                &blocker
            );
            use NonsyncBlocker as B;
            match blocker {
                B::ComponentExit => drop(component),
                B::Inconsistent => return Err(Se::InconsistentProtoComponent(proto_component_id)),
                B::SyncBlockStart => {
                    branching_proto_components
                        .insert(proto_component_id, BranchingProtoComponent::initial(component));
                }
            }
        }
        log!(
            cu.logger,
            "All {} proto components are now done with Nonsync phase",
            branching_proto_components.len(),
        );

        // NOTE: all msgs in outbox are of form (Getter, Payload)
        let mut payloads_to_get: Vec<(PortId, SendPayloadMsg)> = vec![];

        // create the solution storage
        let mut solution_storage = {
            let n = std::iter::once(Route::LocalComponent(ComponentId::Native));
            let c =
                cu.proto_components.keys().map(|&id| Route::LocalComponent(ComponentId::Proto(id)));
            let e = comm.neighborhood.children.iter().map(|&index| Route::Endpoint { index });
            SolutionStorage::new(n.chain(c).chain(e))
        };
        log!(cu.logger, "Solution storage initialized");

        // 2. kick off the native
        log!(
            cu.logger,
            "Translating {} native batches into branches...",
            comm.native_batches.len()
        );
        let mut branching_native = BranchingNative { branches: Default::default() };
        'native_branches: for (index, NativeBatch { to_get, to_put }) in
            comm.native_batches.drain(..).enumerate()
        {
            let predicate = {
                let mut predicate = Predicate::default();
                // assign trues for ports that fire
                let firing_ports: HashSet<PortId> =
                    to_get.iter().chain(to_put.keys()).copied().collect();
                for &port in to_get.iter().chain(to_put.keys()) {
                    let var = cu.port_info.firing_var_for(port);
                    predicate.assigned.insert(var, true);
                }
                // assign falses for silent ports
                for &port in cu.native_ports.difference(&firing_ports) {
                    let var = cu.port_info.firing_var_for(port);
                    if let Some(true) = predicate.assigned.insert(var, false) {
                        log!(cu.logger, "Native branch index={} contains internal inconsistency wrt. {:?}. Skipping", index, var);
                        continue 'native_branches;
                    }
                }
                predicate
            };
            log!(cu.logger, "Native branch index={:?} has consistent {:?}", index, &predicate);

            // put all messages
            for (putter, payload) in to_put {
                let msg = SendPayloadMsg { predicate: predicate.clone(), payload };
                log!(cu.logger, "Native branch {} sending msg {:?}", index, &msg);
                payloads_to_get.putter_send(cu, putter, msg)?;
            }
            if to_get.is_empty() {
                log!(
                    cu.logger,
                    "Native submitting solution for batch {} with {:?}",
                    index,
                    &predicate
                );
                solution_storage.submit_and_digest_subtree_solution(
                    &mut *cu.logger,
                    Route::LocalComponent(ComponentId::Native),
                    predicate.clone(),
                );
            }
            let branch = NativeBranch { index, gotten: Default::default(), to_get };
            if let Some(existing) = branching_native.branches.insert(predicate, branch) {
                return Err(Se::IndistinguishableBatches([index, existing.index]));
            }
        }
        // restore the invariant
        comm.native_batches.push(Default::default());
        let decision = Self::sync_reach_decision(
            cu,
            comm,
            &mut branching_native,
            &mut branching_proto_components,
            solution_storage,
            payloads_to_get,
            deadline,
        )?;
        log!(cu.logger, "Committing to decision {:?}!", &decision);

        // propagate the decision to children
        let msg = Msg::CommMsg(CommMsg {
            round_index: comm.round_index,
            contents: CommMsgContents::Announce { decision: decision.clone() },
        });
        log!(
            cu.logger,
            "Announcing decision {:?} through child endpoints {:?}",
            &msg,
            &comm.neighborhood.children
        );
        for &child in comm.neighborhood.children.iter() {
            comm.endpoint_manager.send_to_comms(child, &msg)?;
        }
        let ret = match decision {
            Decision::Failure => {
                // dropping {branching_proto_components, branching_native}
                Err(Se::RoundFailure)
            }
            Decision::Success(predicate) => {
                // commit changes to component states
                cu.proto_components.clear();
                cu.proto_components.extend(
                    // consume branching proto components
                    branching_proto_components
                        .into_iter()
                        .map(|(id, bpc)| (id, bpc.collapse_with(&predicate))),
                );
                log!(
                    cu.logger,
                    "End round with (updated) component states {:?}",
                    cu.proto_components.keys()
                );
                // consume native
                Ok(Some(branching_native.collapse_with(&predicate)))
            }
        };
        log!(cu.logger, "Sync round ending! Cleaning up");
        // dropping {solution_storage, payloads_to_get}
        ret
    }

    fn sync_reach_decision(
        cu: &mut ConnectorUnphased,
        comm: &mut ConnectorCommunication,
        branching_native: &mut BranchingNative,
        branching_proto_components: &mut HashMap<ProtoComponentId, BranchingProtoComponent>,
        mut solution_storage: SolutionStorage,
        mut payloads_to_get: Vec<(PortId, SendPayloadMsg)>,
        mut deadline: Option<Instant>,
    ) -> Result<Decision, SyncError> {
        let mut already_requested_failure = false;
        if branching_native.branches.is_empty() {
            log!(cu.logger, "Native starts with no branches! Failure!");
            match comm.neighborhood.parent {
                Some(parent) => {
                    if already_requested_failure.replace_with_true() {
                        Self::request_failure(cu, comm, parent)?
                    } else {
                        log!(cu.logger, "Already requested failure");
                    }
                }
                None => {
                    log!(cu.logger, "No parent. Deciding on failure");
                    return Ok(Decision::Failure);
                }
            }
        }
        log!(cu.logger, "Done translating native batches into branches");

        // run all proto components to their sync blocker
        log!(
            cu.logger,
            "Running all {} proto components to their sync blocker...",
            branching_proto_components.len()
        );
        for (&proto_component_id, proto_component) in branching_proto_components.iter_mut() {
            let BranchingProtoComponent { ports, branches } = proto_component;
            let mut swap = HashMap::default();
            let mut blocked = HashMap::default();
            // drain from branches --> blocked
            let cd = CyclicDrainer::new(branches, &mut swap, &mut blocked);
            BranchingProtoComponent::drain_branches_to_blocked(
                cd,
                cu,
                &mut solution_storage,
                &mut payloads_to_get,
                proto_component_id,
                ports,
            )?;
            // swap the blocked branches back
            std::mem::swap(&mut blocked, branches);
            if branches.is_empty() {
                log!(cu.logger, "{:?} has become inconsistent!", proto_component_id);
                if let Some(parent) = comm.neighborhood.parent {
                    if already_requested_failure.replace_with_true() {
                        Self::request_failure(cu, comm, parent)?
                    } else {
                        log!(cu.logger, "Already requested failure");
                    }
                } else {
                    log!(cu.logger, "As the leader, deciding on timeout");
                    return Ok(Decision::Failure);
                }
            }
        }
        log!(cu.logger, "All proto components are blocked");

        log!(cu.logger, "Entering decision loop...");
        comm.endpoint_manager.undelay_all();
        'undecided: loop {
            // drain payloads_to_get, sending them through endpoints / feeding them to components
            log!(cu.logger, "Decision loop! have {} messages to recv", payloads_to_get.len());
            while let Some((getter, send_payload_msg)) = payloads_to_get.pop() {
                assert!(cu.port_info.polarities.get(&getter).copied() == Some(Getter));
                let route = cu.port_info.routes.get(&getter);
                log!(cu.logger, "Routing msg {:?} to {:?}", &send_payload_msg, &route);
                match route {
                    None => {
                        log!(
                            cu.logger,
                            "Delivery to getter {:?} msg {:?} failed. Physical route unmapped!",
                            getter,
                            &send_payload_msg
                        );
                    }
                    Some(Route::Endpoint { index }) => {
                        let msg = Msg::CommMsg(CommMsg {
                            round_index: comm.round_index,
                            contents: CommMsgContents::SendPayload(send_payload_msg),
                        });
                        comm.endpoint_manager.send_to_comms(*index, &msg)?;
                    }
                    Some(Route::LocalComponent(ComponentId::Native)) => branching_native.feed_msg(
                        cu,
                        &mut solution_storage,
                        // &mut Pay
                        getter,
                        &send_payload_msg,
                    ),
                    Some(Route::LocalComponent(ComponentId::Proto(proto_component_id))) => {
                        if let Some(branching_component) =
                            branching_proto_components.get_mut(proto_component_id)
                        {
                            let proto_component_id = *proto_component_id;
                            // let ConnectorUnphased { port_info, proto_description, .. } = cu;
                            branching_component.feed_msg(
                                cu,
                                &mut solution_storage,
                                proto_component_id,
                                &mut payloads_to_get,
                                getter,
                                &send_payload_msg,
                            )?;
                            if branching_component.branches.is_empty() {
                                log!(
                                    cu.logger,
                                    "{:?} has become inconsistent!",
                                    proto_component_id
                                );
                                if let Some(parent) = comm.neighborhood.parent {
                                    if already_requested_failure.replace_with_true() {
                                        Self::request_failure(cu, comm, parent)?
                                    } else {
                                        log!(cu.logger, "Already requested failure");
                                    }
                                } else {
                                    log!(cu.logger, "As the leader, deciding on timeout");
                                    return Ok(Decision::Failure);
                                }
                            }
                        } else {
                            log!(
                                cu.logger,
                                "Delivery to getter {:?} msg {:?} failed because {:?} isn't here",
                                getter,
                                &send_payload_msg,
                                proto_component_id
                            );
                        }
                    }
                }
            }

            // check if we have a solution yet
            log!(cu.logger, "Check if we have any local decisions...");
            for solution in solution_storage.iter_new_local_make_old() {
                log!(cu.logger, "New local decision with solution {:?}...", &solution);
                match comm.neighborhood.parent {
                    Some(parent) => {
                        log!(cu.logger, "Forwarding to my parent {:?}", parent);
                        let suggestion = Decision::Success(solution);
                        let msg = Msg::CommMsg(CommMsg {
                            round_index: comm.round_index,
                            contents: CommMsgContents::Suggest { suggestion },
                        });
                        comm.endpoint_manager.send_to_comms(parent, &msg)?;
                    }
                    None => {
                        log!(cu.logger, "No parent. Deciding on solution {:?}", &solution);
                        return Ok(Decision::Success(solution));
                    }
                }
            }

            // stuck! make progress by receiving a msg
            // try recv messages arriving through endpoints
            log!(cu.logger, "No decision yet. Let's recv an endpoint msg...");
            {
                let (endpoint_index, msg) = loop {
                    match comm.endpoint_manager.try_recv_any_comms(&mut *cu.logger, deadline)? {
                        None => {
                            log!(cu.logger, "Reached user-defined deadling without decision...");
                            if let Some(parent) = comm.neighborhood.parent {
                                if already_requested_failure.replace_with_true() {
                                    Self::request_failure(cu, comm, parent)?
                                } else {
                                    log!(cu.logger, "Already requested failure");
                                }
                            } else {
                                log!(cu.logger, "As the leader, deciding on timeout");
                                return Ok(Decision::Failure);
                            }
                            deadline = None;
                        }
                        Some((endpoint_index, msg)) => break (endpoint_index, msg),
                    }
                };
                log!(cu.logger, "Received from endpoint {} msg {:?}", endpoint_index, &msg);
                let comm_msg_contents = match msg {
                    Msg::SetupMsg(..) => {
                        log!(cu.logger, "Discarding setup message; that phase is over");
                        continue 'undecided;
                    }
                    Msg::CommMsg(comm_msg) => match comm_msg.round_index.cmp(&comm.round_index) {
                        Ordering::Equal => comm_msg.contents,
                        Ordering::Less => {
                            log!(
                                cu.logger,
                                "We are in round {}, but msg is for round {}. Discard",
                                comm_msg.round_index,
                                comm.round_index,
                            );
                            drop(comm_msg);
                            continue 'undecided;
                        }
                        Ordering::Greater => {
                            log!(
                                cu.logger,
                                "We are in round {}, but msg is for round {}. Buffer",
                                comm_msg.round_index,
                                comm.round_index,
                            );
                            comm.endpoint_manager
                                .delayed_messages
                                .push((endpoint_index, Msg::CommMsg(comm_msg)));
                            continue 'undecided;
                        }
                    },
                };
                match comm_msg_contents {
                    CommMsgContents::SendPayload(send_payload_msg) => {
                        let getter =
                            comm.endpoint_manager.endpoint_exts[endpoint_index].getter_for_incoming;
                        assert!(cu.port_info.polarities.get(&getter) == Some(&Getter));
                        log!(
                            cu.logger,
                            "Msg routed to getter port {:?}. Buffer for recv loop",
                            getter,
                        );
                        payloads_to_get.push((getter, send_payload_msg));
                    }
                    CommMsgContents::Suggest { suggestion } => {
                        // only accept this control msg through a child endpoint
                        if comm.neighborhood.children.contains(&endpoint_index) {
                            match suggestion {
                                Decision::Success(predicate) => {
                                    // child solution contributes to local solution
                                    log!(cu.logger, "Child provided solution {:?}", &predicate);
                                    let route = Route::Endpoint { index: endpoint_index };
                                    solution_storage.submit_and_digest_subtree_solution(
                                        &mut *cu.logger,
                                        route,
                                        predicate,
                                    );
                                }
                                Decision::Failure => {
                                    match comm.neighborhood.parent {
                                        None => {
                                            log!(cu.logger, "I decide on my child's failure");
                                            break 'undecided Ok(Decision::Failure);
                                        }
                                        Some(parent) => {
                                            log!(cu.logger, "Forwarding failure through my parent endpoint {:?}", parent);
                                            if already_requested_failure.replace_with_true() {
                                                Self::request_failure(cu, comm, parent)?
                                            } else {
                                                log!(cu.logger, "Already requested failure");
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            log!(
                                cu.logger,
                                "Discarding suggestion {:?} from non-child endpoint idx {:?}",
                                &suggestion,
                                endpoint_index
                            );
                        }
                    }
                    CommMsgContents::Announce { decision } => {
                        if Some(endpoint_index) == comm.neighborhood.parent {
                            // adopt this decision
                            return Ok(decision);
                        } else {
                            log!(
                                cu.logger,
                                "Discarding announcement {:?} from non-parent endpoint idx {:?}",
                                &decision,
                                endpoint_index
                            );
                        }
                    }
                }
            }
            log!(cu.logger, "Endpoint msg recv done");
        }
    }
    fn request_failure(
        cu: &mut ConnectorUnphased,
        comm: &mut ConnectorCommunication,
        parent: usize,
    ) -> Result<(), SyncError> {
        log!(cu.logger, "Forwarding to my parent {:?}", parent);
        let suggestion = Decision::Failure;
        let msg = Msg::CommMsg(CommMsg {
            round_index: comm.round_index,
            contents: CommMsgContents::Suggest { suggestion },
        });
        comm.endpoint_manager.send_to_comms(parent, &msg)
    }
}
impl BranchingNative {
    fn feed_msg(
        &mut self,
        cu: &mut ConnectorUnphased,
        solution_storage: &mut SolutionStorage,
        getter: PortId,
        send_payload_msg: &SendPayloadMsg,
    ) {
        log!(cu.logger, "feeding native getter {:?} {:?}", getter, &send_payload_msg);
        assert!(cu.port_info.polarities.get(&getter).copied() == Some(Getter));
        let mut draining = HashMap::default();
        let finished = &mut self.branches;
        std::mem::swap(&mut draining, finished);
        for (predicate, mut branch) in draining.drain() {
            log!(cu.logger, "visiting native branch {:?} with {:?}", &branch, &predicate);
            // check if this branch expects to receive it
            let var = cu.port_info.firing_var_for(getter);
            let mut feed_branch = |branch: &mut NativeBranch, predicate: &Predicate| {
                let was = branch.gotten.insert(getter, send_payload_msg.payload.clone());
                assert!(was.is_none());
                branch.to_get.remove(&getter);
                if branch.to_get.is_empty() {
                    let route = Route::LocalComponent(ComponentId::Native);
                    solution_storage.submit_and_digest_subtree_solution(
                        &mut *cu.logger,
                        route,
                        predicate.clone(),
                    );
                }
            };
            if predicate.query(var) != Some(true) {
                // optimization. Don't bother trying this branch
                log!(
                    cu.logger,
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
                        cu.logger,
                        "skipping branch with {:?} that doesn't want the message (slowpath)",
                        &predicate
                    );
                    finished.insert(predicate, branch);
                }
                Csr::Equivalent | Csr::FormerNotLatter => {
                    // retain the existing predicate, but add this payload
                    feed_branch(&mut branch, &predicate);
                    log!(cu.logger, "branch pred covers it! Accept the msg");
                    finished.insert(predicate, branch);
                }
                Csr::LatterNotFormer => {
                    // fork branch, give fork the message and payload predicate. original branch untouched
                    let mut branch2 = branch.clone();
                    let predicate2 = send_payload_msg.predicate.clone();
                    feed_branch(&mut branch2, &predicate2);
                    log!(
                        cu.logger,
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
                        cu.logger,
                        "new subsuming pred created {:?}. forking and feeding",
                        &predicate2
                    );
                    finished.insert(predicate, branch);
                    finished.insert(predicate2, branch2);
                }
            }
        }
    }
    fn collapse_with(self, solution_predicate: &Predicate) -> RoundOk {
        for (branch_predicate, branch) in self.branches {
            if solution_predicate.satisfies(&branch_predicate) {
                let NativeBranch { index, gotten, .. } = branch;
                return RoundOk { batch_index: index, gotten };
            }
        }
        panic!("Native had no branches matching pred {:?}", solution_predicate);
    }
}
// |putter, m| {
//     let getter = *cu.port_info.peers.get(&putter).unwrap();
//     payloads_to_get.push((getter, m));
// },
impl BranchingProtoComponent {
    fn drain_branches_to_blocked(
        cd: CyclicDrainer<Predicate, ProtoComponentBranch>,
        cu: &mut ConnectorUnphased,
        solution_storage: &mut SolutionStorage,
        payload_msg_sender: &mut impl PayloadMsgSender,
        proto_component_id: ProtoComponentId,
        ports: &HashSet<PortId>,
    ) -> Result<(), SyncError> {
        cd.cylic_drain(|mut predicate, mut branch, mut drainer| {
            let mut ctx = SyncProtoContext {
                logger: &mut *cu.logger,
                predicate: &predicate,
                port_info: &cu.port_info,
                inbox: &branch.inbox,
            };
            let blocker = branch.state.sync_run(&mut ctx, &cu.proto_description);
            log!(
                cu.logger,
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
                        let var = cu.port_info.firing_var_for(port);
                        predicate.assigned.entry(var).or_insert(false);
                    }
                    // submit solution for this component
                    solution_storage.submit_and_digest_subtree_solution(
                        &mut *cu.logger,
                        Route::LocalComponent(ComponentId::Proto(proto_component_id)),
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
                    let var = cu.port_info.firing_var_for(port);
                    assert!(predicate.query(var).is_none());
                    // keep forks in "unblocked"
                    drainer.add_input(predicate.clone().inserted(var, false), branch.clone());
                    drainer.add_input(predicate.inserted(var, true), branch);
                }
                B::PutMsg(putter, payload) => {
                    // sanity check
                    assert_eq!(Some(&Putter), cu.port_info.polarities.get(&putter));
                    // overwrite assignment
                    let var = cu.port_info.firing_var_for(putter);
                    let was = predicate.assigned.insert(var, true);
                    if was == Some(false) {
                        log!(cu.logger, "Proto component {:?} tried to PUT on port {:?} when pred said var {:?}==Some(false). inconsistent!", proto_component_id, putter, var);
                        // discard forever
                        drop((predicate, branch));
                    } else {
                        // keep in "unblocked"
                        log!(cu.logger, "Proto component {:?} putting payload {:?} on port {:?} (using var {:?})", proto_component_id, &payload, putter, var);
                        let msg = SendPayloadMsg { predicate: predicate.clone(), payload };
                        payload_msg_sender.putter_send(cu, putter, msg)?;
                        drainer.add_input(predicate, branch);
                    }
                }
            }
            Ok(())
        })
    }
    fn feed_msg(
        &mut self,
        cu: &mut ConnectorUnphased,
        solution_storage: &mut SolutionStorage,
        proto_component_id: ProtoComponentId,
        payload_msg_sender: &mut impl PayloadMsgSender,
        getter: PortId,
        send_payload_msg: &SendPayloadMsg,
    ) -> Result<(), SyncError> {
        let logger = &mut *cu.logger;
        log!(
            logger,
            "feeding proto component {:?} getter {:?} {:?}",
            proto_component_id,
            getter,
            &send_payload_msg
        );
        let BranchingProtoComponent { branches, ports } = self;
        let mut unblocked = HashMap::default();
        let mut blocked = HashMap::default();
        // partition drain from branches -> {unblocked, blocked}
        log!(logger, "visiting {} blocked branches...", branches.len());
        for (predicate, mut branch) in branches.drain() {
            use CommonSatResult as Csr;
            log!(logger, "visiting branch with pred {:?}", &predicate);
            match predicate.common_satisfier(&send_payload_msg.predicate) {
                Csr::Nonexistant => {
                    // this branch does not receive the message
                    log!(logger, "skipping branch");
                    blocked.insert(predicate, branch);
                }
                Csr::Equivalent | Csr::FormerNotLatter => {
                    // retain the existing predicate, but add this payload
                    log!(logger, "feeding this branch without altering its predicate");
                    branch.feed_msg(getter, send_payload_msg.payload.clone());
                    unblocked.insert(predicate, branch);
                }
                Csr::LatterNotFormer => {
                    // fork branch, give fork the message and payload predicate. original branch untouched
                    log!(logger, "Forking this branch, giving it the predicate of the msg");
                    let mut branch2 = branch.clone();
                    let predicate2 = send_payload_msg.predicate.clone();
                    branch2.feed_msg(getter, send_payload_msg.payload.clone());
                    blocked.insert(predicate, branch);
                    unblocked.insert(predicate2, branch2);
                }
                Csr::New(predicate2) => {
                    // fork branch, give fork the message and the new predicate. original branch untouched
                    log!(logger, "Forking this branch with new predicate {:?}", &predicate2);
                    let mut branch2 = branch.clone();
                    branch2.feed_msg(getter, send_payload_msg.payload.clone());
                    blocked.insert(predicate, branch);
                    unblocked.insert(predicate2, branch2);
                }
            }
        }
        log!(logger, "blocked {:?} unblocked {:?}", blocked.len(), unblocked.len());
        // drain from unblocked --> blocked
        let mut swap = HashMap::default();
        let cd = CyclicDrainer::new(&mut unblocked, &mut swap, &mut blocked);
        BranchingProtoComponent::drain_branches_to_blocked(
            cd,
            cu,
            solution_storage,
            payload_msg_sender,
            proto_component_id,
            ports,
        )?;
        // swap the blocked branches back
        std::mem::swap(&mut blocked, branches);
        log!(cu.logger, "component settles down with branches: {:?}", branches.keys());
        Ok(())
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
    fn reset(&mut self, subtree_ids: impl Iterator<Item = Route>) {
        self.subtree_id_to_index.clear();
        self.subtree_solutions.clear();
        self.old_local.clear();
        self.new_local.clear();
        for key in subtree_ids {
            self.subtree_id_to_index.insert(key, self.subtree_solutions.len());
            self.subtree_solutions.push(Default::default())
        }
    }
    // pub(crate) fn peek_new_locals(&self) -> impl Iterator<Item = &Predicate> + '_ {
    //     self.new_local.iter()
    // }
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
impl PayloadMsgSender for Vec<(PortId, SendPayloadMsg)> {
    fn putter_send(
        &mut self,
        cu: &mut ConnectorUnphased,
        putter: PortId,
        msg: SendPayloadMsg,
    ) -> Result<(), SyncError> {
        if let Some(&getter) = cu.port_info.peers.get(&putter) {
            self.push((getter, msg));
            Ok(())
        } else {
            Err(SyncError::MalformedStateError(MalformedStateError::GetterUnknownFor { putter }))
        }
    }
}
impl SyncProtoContext<'_> {
    pub(crate) fn is_firing(&mut self, port: PortId) -> Option<bool> {
        let var = self.port_info.firing_var_for(port);
        self.predicate.query(var)
    }
    pub(crate) fn read_msg(&mut self, port: PortId) -> Option<&Payload> {
        self.inbox.get(&port)
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
            self.port_info.routes.insert(*port, Route::LocalComponent(ComponentId::Proto(new_id)));
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
        let route = Route::LocalComponent(ComponentId::Proto(self.proto_component_id));
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
impl ProtoComponentBranch {
    fn feed_msg(&mut self, getter: PortId, payload: Payload) {
        let was = self.inbox.insert(getter, payload);
        assert!(was.is_none())
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
    fn cylic_drain<E>(
        self,
        mut func: impl FnMut(K, V, CyclicDrainInner<'_, K, V>) -> Result<(), E>,
    ) -> Result<(), E> {
        let Self { input, inner: CyclicDrainInner { swap, output } } = self;
        // assert!(swap.is_empty());
        while !input.is_empty() {
            for (k, v) in input.drain() {
                func(k, v, CyclicDrainInner { swap, output })?
            }
            std::mem::swap(input, swap);
        }
        Ok(())
    }
}
