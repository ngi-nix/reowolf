use super::*;
use crate::common::*;

////////////////
#[derive(Default)]
struct GetterBuffer {
    getters_and_sends: Vec<(PortId, SendPayloadMsg)>,
}
struct RoundCtx {
    solution_storage: SolutionStorage,
    spec_var_stream: SpecVarStream,
    getter_buffer: GetterBuffer,
    deadline: Option<Instant>,
}
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
    // this pair acts as SubtreeId -> HashSet<Predicate> which is friendlier to iteration
    subtree_solutions: Vec<HashSet<Predicate>>,
    subtree_id_to_index: HashMap<SubtreeId, usize>,
}
#[derive(Debug)]
struct BranchingProtoComponent {
    ports: HashSet<PortId>,
    branches: HashMap<Predicate, ProtoComponentBranch>,
}
#[derive(Debug, Clone)]
struct ProtoComponentBranch {
    did_put_or_get: HashSet<PortId>,
    inbox: HashMap<PortId, Payload>,
    state: ComponentState,
    untaken_choice: Option<u16>,
    ended: bool,
}
struct CyclicDrainer<'a, K: Eq + Hash, V> {
    input: &'a mut HashMap<K, V>,
    inner: CyclicDrainInner<'a, K, V>,
}
struct CyclicDrainInner<'a, K: Eq + Hash, V> {
    swap: &'a mut HashMap<K, V>,
    output: &'a mut HashMap<K, V>,
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
impl RoundCtxTrait for RoundCtx {
    fn get_deadline(&self) -> &Option<Instant> {
        &self.deadline
    }
    fn getter_add(&mut self, getter: PortId, msg: SendPayloadMsg) {
        self.getter_buffer.getter_add(getter, msg)
    }
}
impl Connector {
    fn get_comm_mut(&mut self) -> Option<&mut ConnectorCommunication> {
        if let ConnectorPhased::Communication(comm) = &mut self.phased {
            Some(comm)
        } else {
            None
        }
    }
    // pub(crate) fn get_mut_udp_sock(&mut self, index: usize) -> Option<&mut UdpSocket> {
    //     let sock = &mut self
    //         .get_comm_mut()?
    //         .endpoint_manager
    //         .udp_endpoint_store
    //         .endpoint_exts
    //         .get_mut(index)?
    //         .sock;
    //     Some(sock)
    // }
    pub fn gotten(&mut self, port: PortId) -> Result<&Payload, GottenError> {
        use GottenError as Ge;
        let comm = self.get_comm_mut().ok_or(Ge::NoPreviousRound)?;
        match &comm.round_result {
            Err(_) => Err(Ge::PreviousSyncFailed),
            Ok(None) => Err(Ge::NoPreviousRound),
            Ok(Some(round_ok)) => round_ok.gotten.get(&port).ok_or(Ge::PortDidntGet),
        }
    }
    pub fn next_batch(&mut self) -> Result<usize, WrongStateError> {
        // returns index of new batch
        let comm = self.get_comm_mut().ok_or(WrongStateError)?;
        comm.native_batches.push(Default::default());
        Ok(comm.native_batches.len() - 1)
    }
    fn port_op_access(
        &mut self,
        port: PortId,
        expect_polarity: Polarity,
    ) -> Result<&mut NativeBatch, PortOpError> {
        use PortOpError as Poe;
        let Self { unphased, phased } = self;
        if !unphased.native_ports.contains(&port) {
            return Err(Poe::PortUnavailable);
        }
        match unphased.port_info.polarities.get(&port) {
            Some(p) if *p == expect_polarity => {}
            Some(_) => return Err(Poe::WrongPolarity),
            None => return Err(Poe::UnknownPolarity),
        }
        match phased {
            ConnectorPhased::Setup { .. } => Err(Poe::NotConnected),
            ConnectorPhased::Communication(comm) => {
                let batch = comm.native_batches.last_mut().unwrap(); // length >= 1 is invariant
                Ok(batch)
            }
        }
    }
    pub fn put(&mut self, port: PortId, payload: Payload) -> Result<(), PortOpError> {
        use PortOpError as Poe;
        let batch = self.port_op_access(port, Putter)?;
        if batch.to_put.contains_key(&port) {
            Err(Poe::MultipleOpsOnPort)
        } else {
            batch.to_put.insert(port, payload);
            Ok(())
        }
    }
    pub fn get(&mut self, port: PortId) -> Result<(), PortOpError> {
        use PortOpError as Poe;
        let batch = self.port_op_access(port, Getter)?;
        if batch.to_get.insert(port) {
            Ok(())
        } else {
            Err(Poe::MultipleOpsOnPort)
        }
    }
    // entrypoint for caller. overwrites round result enum, and returns what happened
    pub fn sync(&mut self, timeout: Option<Duration>) -> Result<usize, SyncError> {
        let Self { unphased: cu, phased } = self;
        match phased {
            ConnectorPhased::Setup { .. } => Err(SyncError::NotConnected),
            ConnectorPhased::Communication(comm) => {
                match &comm.round_result {
                    Err(SyncError::Unrecoverable(e)) => {
                        log!(cu.logger, "Attempted to start sync round, but previous error {:?} was unrecoverable!", e);
                        return Err(SyncError::Unrecoverable(e.clone()));
                    }
                    _ => {}
                }
                comm.round_result = Self::connected_sync(cu, comm, timeout);
                comm.round_index += 1;
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
        //////////////////////////////////
        use SyncError as Se;
        //////////////////////////////////
        log!(
            cu.logger,
            "~~~ SYNC called with timeout {:?}; starting round {}",
            &timeout,
            comm.round_index
        );

        // 1. run all proto components to Nonsync blockers
        // NOTE: original components are immutable until Decision::Success
        let mut branching_proto_components =
            HashMap::<ProtoComponentId, BranchingProtoComponent>::default();
        let mut unrun_components: Vec<(ProtoComponentId, ProtoComponent)> =
            cu.proto_components.iter().map(|(&k, v)| (k, v.clone())).collect();
        log!(cu.logger, "Nonsync running {} proto components...", unrun_components.len());
        // drains unrun_components, and populates branching_proto_components.
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

        // Create temp structures needed for the synchronous phase of the round
        let mut rctx = RoundCtx {
            solution_storage: {
                let n = std::iter::once(SubtreeId::LocalComponent(ComponentId::Native));
                let c = cu
                    .proto_components
                    .keys()
                    .map(|&id| SubtreeId::LocalComponent(ComponentId::Proto(id)));
                let e = comm
                    .neighborhood
                    .children
                    .iter()
                    .map(|&index| SubtreeId::NetEndpoint { index });
                let subtree_id_iter = n.chain(c).chain(e);
                log!(
                    cu.logger,
                    "Children in subtree are: {:?}",
                    subtree_id_iter.clone().collect::<Vec<_>>()
                );
                SolutionStorage::new(subtree_id_iter)
            },
            spec_var_stream: cu.id_manager.new_spec_var_stream(),
            getter_buffer: Default::default(),
            deadline: timeout.map(|to| Instant::now() + to),
        };
        log!(cu.logger, "Round context structure initialized");

        // Explore all native branches eagerly. Find solutions, buffer messages, etc.
        log!(
            cu.logger,
            "Translating {} native batches into branches...",
            comm.native_batches.len()
        );
        let native_branch_spec_var = rctx.spec_var_stream.next();
        log!(cu.logger, "Native branch spec var is {:?}", native_branch_spec_var);
        let mut branching_native = BranchingNative { branches: Default::default() };
        'native_branches: for ((native_branch, index), branch_spec_val) in
            comm.native_batches.drain(..).zip(0..).zip(SpecVal::iter_domain())
        {
            let NativeBatch { to_get, to_put } = native_branch;
            let predicate = {
                let mut predicate = Predicate::default();
                // assign trues for ports that fire
                let firing_ports: HashSet<PortId> =
                    to_get.iter().chain(to_put.keys()).copied().collect();
                for &port in to_get.iter().chain(to_put.keys()) {
                    let var = cu.port_info.spec_var_for(port);
                    predicate.assigned.insert(var, SpecVal::FIRING);
                }
                // assign falses for all silent (not firing) ports
                for &port in cu.native_ports.difference(&firing_ports) {
                    let var = cu.port_info.spec_var_for(port);
                    if let Some(SpecVal::FIRING) = predicate.assigned.insert(var, SpecVal::SILENT) {
                        log!(cu.logger, "Native branch index={} contains internal inconsistency wrt. {:?}. Skipping", index, var);
                        continue 'native_branches;
                    }
                }
                // this branch is consistent. distinguish it with a unique var:val mapping and proceed
                predicate.inserted(native_branch_spec_var, branch_spec_val)
            };
            log!(cu.logger, "Native branch index={:?} has consistent {:?}", index, &predicate);
            // send all outgoing messages (by buffering them)
            for (putter, payload) in to_put {
                let msg = SendPayloadMsg { predicate: predicate.clone(), payload };
                log!(cu.logger, "Native branch {} sending msg {:?}", index, &msg);
                rctx.getter_buffer.putter_add(cu, putter, msg);
            }
            let branch = NativeBranch { index, gotten: Default::default(), to_get };
            if branch.is_ended() {
                log!(
                    cu.logger,
                    "Native submitting solution for batch {} with {:?}",
                    index,
                    &predicate
                );
                rctx.solution_storage.submit_and_digest_subtree_solution(
                    &mut *cu.logger,
                    SubtreeId::LocalComponent(ComponentId::Native),
                    predicate.clone(),
                );
            }
            if let Some(_) = branching_native.branches.insert(predicate, branch) {
                // thanks to the native_branch_spec_var, each batch has a distinct predicate
                unreachable!()
            }
        }
        // restore the invariant: !native_batches.is_empty()
        comm.native_batches.push(Default::default());

        comm.endpoint_manager.udp_endpoints_round_start(&mut *cu.logger, &mut rctx.spec_var_stream);
        // Call to another big method; keep running this round until a distributed decision is reached
        let decision = Self::sync_reach_decision(
            cu,
            comm,
            &mut branching_native,
            &mut branching_proto_components,
            &mut rctx,
        )?;
        log!(cu.logger, "Committing to decision {:?}!", &decision);
        comm.endpoint_manager.udp_endpoints_round_end(&mut *cu.logger, &decision)?;

        // propagate the decision to children
        let msg = Msg::CommMsg(CommMsg {
            round_index: comm.round_index,
            contents: CommMsgContents::CommCtrl(CommCtrlMsg::Announce {
                decision: decision.clone(),
            }),
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
                Ok(Some(branching_native.collapse_with(&mut *cu.logger, &predicate)))
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
        rctx: &mut RoundCtx,
    ) -> Result<Decision, UnrecoverableSyncError> {
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
            // initially, no components have .ended==true
            let mut blocked = HashMap::default();
            // drain from branches --> blocked
            let cd = CyclicDrainer::new(branches, &mut swap, &mut blocked);
            BranchingProtoComponent::drain_branches_to_blocked(
                cd,
                cu,
                rctx,
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
            log!(cu.logger, "Decision loop! have {} messages to recv", rctx.getter_buffer.len());
            while let Some((getter, send_payload_msg)) = rctx.getter_buffer.pop() {
                assert!(cu.port_info.polarities.get(&getter).copied() == Some(Getter));
                let route = cu.port_info.routes.get(&getter);
                log!(
                    cu.logger,
                    "Routing msg {:?} to {:?} via {:?}",
                    &send_payload_msg,
                    getter,
                    &route
                );
                match route {
                    None => log!(cu.logger, "Delivery failed. Physical route unmapped!"),
                    Some(Route::UdpEndpoint { index }) => {
                        let udp_endpoint_ext =
                            &mut comm.endpoint_manager.udp_endpoint_store.endpoint_exts[*index];
                        let SendPayloadMsg { predicate, payload } = send_payload_msg;
                        log!(cu.logger, "Delivering to udp endpoint index={}", index);
                        udp_endpoint_ext.outgoing_payloads.insert(predicate, payload);
                    }
                    Some(Route::NetEndpoint { index }) => {
                        let msg = Msg::CommMsg(CommMsg {
                            round_index: comm.round_index,
                            contents: CommMsgContents::SendPayload(send_payload_msg),
                        });
                        comm.endpoint_manager.send_to_comms(*index, &msg)?;
                    }
                    Some(Route::LocalComponent(ComponentId::Native)) => branching_native.feed_msg(
                        cu,
                        &mut rctx.solution_storage,
                        getter,
                        &send_payload_msg,
                    ),
                    Some(Route::LocalComponent(ComponentId::Proto(proto_component_id))) => {
                        if let Some(branching_component) =
                            branching_proto_components.get_mut(proto_component_id)
                        {
                            let proto_component_id = *proto_component_id;
                            branching_component.feed_msg(
                                cu,
                                rctx,
                                proto_component_id,
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
            for solution in rctx.solution_storage.iter_new_local_make_old() {
                log!(cu.logger, "New local decision with solution {:?}...", &solution);
                match comm.neighborhood.parent {
                    Some(parent) => {
                        log!(cu.logger, "Forwarding to my parent {:?}", parent);
                        let suggestion = Decision::Success(solution);
                        let msg = Msg::CommMsg(CommMsg {
                            round_index: comm.round_index,
                            contents: CommMsgContents::CommCtrl(CommCtrlMsg::Suggest {
                                suggestion,
                            }),
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
                let (net_index, comm_ctrl_msg): (usize, CommCtrlMsg) =
                    match comm.endpoint_manager.try_recv_any_comms(
                        &mut *cu.logger,
                        &cu.port_info,
                        rctx,
                        comm.round_index,
                    )? {
                        CommRecvOk::NewControlMsg { net_index, msg } => (net_index, msg),
                        CommRecvOk::NewPayloadMsgs => continue 'undecided,
                        CommRecvOk::TimeoutWithoutNew => {
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
                            rctx.deadline = None;
                            continue 'undecided;
                        }
                    };
                log!(
                    cu.logger,
                    "Received from endpoint {} ctrl msg  {:?}",
                    net_index,
                    &comm_ctrl_msg
                );
                match comm_ctrl_msg {
                    CommCtrlMsg::Suggest { suggestion } => {
                        // only accept this control msg through a child endpoint
                        if comm.neighborhood.children.contains(&net_index) {
                            match suggestion {
                                Decision::Success(predicate) => {
                                    // child solution contributes to local solution
                                    log!(cu.logger, "Child provided solution {:?}", &predicate);
                                    let subtree_id = SubtreeId::NetEndpoint { index: net_index };
                                    rctx.solution_storage.submit_and_digest_subtree_solution(
                                        &mut *cu.logger,
                                        subtree_id,
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
                                net_index
                            );
                        }
                    }
                    CommCtrlMsg::Announce { decision } => {
                        if Some(net_index) == comm.neighborhood.parent {
                            // adopt this decision
                            return Ok(decision);
                        } else {
                            log!(
                                cu.logger,
                                "Discarding announcement {:?} from non-parent endpoint idx {:?}",
                                &decision,
                                net_index
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
    ) -> Result<(), UnrecoverableSyncError> {
        log!(cu.logger, "Forwarding to my parent {:?}", parent);
        let suggestion = Decision::Failure;
        let msg = Msg::CommMsg(CommMsg {
            round_index: comm.round_index,
            contents: CommMsgContents::CommCtrl(CommCtrlMsg::Suggest { suggestion }),
        });
        comm.endpoint_manager.send_to_comms(parent, &msg)
    }
}
impl NativeBranch {
    fn is_ended(&self) -> bool {
        self.to_get.is_empty()
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
            let var = cu.port_info.spec_var_for(getter);
            let mut feed_branch = |branch: &mut NativeBranch, predicate: &Predicate| {
                branch.to_get.remove(&getter);
                let was = branch.gotten.insert(getter, send_payload_msg.payload.clone());
                assert!(was.is_none());
                if branch.is_ended() {
                    log!(
                        cu.logger,
                        "new native solution with {:?} is_ended() with gotten {:?}",
                        &predicate,
                        &branch.gotten
                    );
                    let subtree_id = SubtreeId::LocalComponent(ComponentId::Native);
                    solution_storage.submit_and_digest_subtree_solution(
                        &mut *cu.logger,
                        subtree_id,
                        predicate.clone(),
                    );
                } else {
                    log!(
                        cu.logger,
                        "Fed native {:?} still has to_get {:?}",
                        &predicate,
                        &branch.to_get
                    );
                }
            };
            if predicate.query(var) != Some(SpecVal::FIRING) {
                // optimization. Don't bother trying this branch
                log!(
                    cu.logger,
                    "skipping branch with {:?} that doesn't want the message (fastpath)",
                    &predicate
                );
                Self::insert_branch_merging(finished, predicate, branch);
                continue;
            }
            use AssignmentUnionResult as Aur;
            match predicate.assignment_union(&send_payload_msg.predicate) {
                Aur::Nonexistant => {
                    // this branch does not receive the message
                    log!(
                        cu.logger,
                        "skipping branch with {:?} that doesn't want the message (slowpath)",
                        &predicate
                    );
                    Self::insert_branch_merging(finished, predicate, branch);
                }
                Aur::Equivalent | Aur::FormerNotLatter => {
                    // retain the existing predicate, but add this payload
                    feed_branch(&mut branch, &predicate);
                    log!(cu.logger, "branch pred covers it! Accept the msg");
                    Self::insert_branch_merging(finished, predicate, branch);
                }
                Aur::LatterNotFormer => {
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
                    Self::insert_branch_merging(finished, predicate, branch);
                    Self::insert_branch_merging(finished, predicate2, branch2);
                }
                Aur::New(predicate2) => {
                    // fork branch, give fork the message and the new predicate. original branch untouched
                    let mut branch2 = branch.clone();
                    feed_branch(&mut branch2, &predicate2);
                    log!(
                        cu.logger,
                        "new subsuming pred created {:?}. forking and feeding",
                        &predicate2
                    );
                    Self::insert_branch_merging(finished, predicate, branch);
                    Self::insert_branch_merging(finished, predicate2, branch2);
                }
            }
        }
    }
    fn insert_branch_merging(
        branches: &mut HashMap<Predicate, NativeBranch>,
        predicate: Predicate,
        mut branch: NativeBranch,
    ) {
        let e = branches.entry(predicate);
        use std::collections::hash_map::Entry;
        match e {
            Entry::Vacant(ev) => {
                // no existing branch present. We insert it no problem. (The most common case)
                ev.insert(branch);
            }
            Entry::Occupied(mut eo) => {
                // Oh dear, there is already a branch with this predicate.
                // Rather than choosing either branch, we MERGE them.
                // This means taking the UNION of their .gotten and the INTERSECTION of their .to_get
                let old = eo.get_mut();
                for (k, v) in branch.gotten.drain() {
                    if old.gotten.insert(k, v).is_none() {
                        // added a gotten element in `branch` not already in `old`
                        old.to_get.remove(&k);
                    }
                }
            }
        }
    }
    fn collapse_with(self, logger: &mut dyn Logger, solution_predicate: &Predicate) -> RoundOk {
        log!(
            logger,
            "Collapsing native with {} branch preds {:?}",
            self.branches.len(),
            self.branches.keys()
        );
        for (branch_predicate, branch) in self.branches {
            log!(
                logger,
                "Considering native branch {:?} with to_get {:?} gotten {:?}",
                &branch_predicate,
                &branch.to_get,
                &branch.gotten
            );
            if branch.is_ended() && branch_predicate.assigns_subset(solution_predicate) {
                let NativeBranch { index, gotten, .. } = branch;
                log!(logger, "Collapsed native has gotten {:?}", &gotten);
                return RoundOk { batch_index: index, gotten };
            }
        }
        panic!("Native had no branches matching pred {:?}", solution_predicate);
    }
}
impl BranchingProtoComponent {
    fn drain_branches_to_blocked(
        cd: CyclicDrainer<Predicate, ProtoComponentBranch>,
        cu: &mut ConnectorUnphased,
        rctx: &mut RoundCtx,
        proto_component_id: ProtoComponentId,
        ports: &HashSet<PortId>,
    ) -> Result<(), UnrecoverableSyncError> {
        cd.cyclic_drain(|mut predicate, mut branch, mut drainer| {
            let mut ctx = SyncProtoContext {
                untaken_choice: &mut branch.untaken_choice,
                logger: &mut *cu.logger,
                predicate: &predicate,
                port_info: &cu.port_info,
                inbox: &branch.inbox,
                did_put_or_get: &mut branch.did_put_or_get,
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
                B::NondetChoice { n } => {
                    let var = rctx.spec_var_stream.next();
                    for val in SpecVal::iter_domain().take(n as usize) {
                        let pred = predicate.clone().inserted(var, val);
                        let mut branch_n = branch.clone();
                        branch_n.untaken_choice = Some(val.0);
                        drainer.add_input(pred, branch_n);
                    }
                }
                B::Inconsistent => {
                    // EXPLICIT inconsistency
                    drop((predicate, branch));
                }
                B::SyncBlockEnd => {
                    // make concrete all variables
                    for port in ports.iter() {
                        let var = cu.port_info.spec_var_for(*port);
                        let should_have_fired = branch.did_put_or_get.contains(port);
                        let val = *predicate.assigned.entry(var).or_insert(SpecVal::SILENT);
                        let did_fire = val == SpecVal::FIRING;
                        if did_fire != should_have_fired {
                            log!(cu.logger, "Inconsistent wrt. port {:?} var {:?} val {:?} did_fire={}, should_have_fired={}", port, var, val, did_fire, should_have_fired);
                            // IMPLICIT inconsistency
                            drop((predicate, branch));
                            return Ok(());
                        }
                    }
                    // submit solution for this component
                    let subtree_id = SubtreeId::LocalComponent(ComponentId::Proto(proto_component_id));
                    rctx.solution_storage.submit_and_digest_subtree_solution(
                        &mut *cu.logger,
                        subtree_id,
                        predicate.clone(),
                    );
                    branch.ended = true;
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
                    let var = cu.port_info.spec_var_for(port);
                    assert!(predicate.query(var).is_none());
                    // keep forks in "unblocked"
                    drainer.add_input(predicate.clone().inserted(var, SpecVal::SILENT), branch.clone());
                    drainer.add_input(predicate.inserted(var, SpecVal::FIRING), branch);
                }
                B::PutMsg(putter, payload) => {
                    // sanity check
                    assert_eq!(Some(&Putter), cu.port_info.polarities.get(&putter));
                    // overwrite assignment
                    let var = cu.port_info.spec_var_for(putter);
                    let was = predicate.assigned.insert(var, SpecVal::FIRING);
                    if was == Some(SpecVal::SILENT) {
                        log!(cu.logger, "Proto component {:?} tried to PUT on port {:?} when pred said var {:?}==Some(false). inconsistent!", proto_component_id, putter, var);
                        // discard forever
                        drop((predicate, branch));
                    } else {
                        // keep in "unblocked"
                        branch.did_put_or_get.insert(putter);
                        log!(cu.logger, "Proto component {:?} putting payload {:?} on port {:?} (using var {:?})", proto_component_id, &payload, putter, var);
                        let msg = SendPayloadMsg { predicate: predicate.clone(), payload };
                        rctx.getter_buffer.putter_add(cu, putter, msg);
                        drainer.add_input(predicate, branch);
                    }
                }
            }
            Ok(())
        })
    }
    fn branch_merge_func(
        mut a: ProtoComponentBranch,
        b: &mut ProtoComponentBranch,
    ) -> ProtoComponentBranch {
        if b.ended && !a.ended {
            a.ended = true;
            std::mem::swap(&mut a, b);
        }
        a
    }
    fn feed_msg(
        &mut self,
        cu: &mut ConnectorUnphased,
        rctx: &mut RoundCtx,
        proto_component_id: ProtoComponentId,
        getter: PortId,
        send_payload_msg: &SendPayloadMsg,
    ) -> Result<(), UnrecoverableSyncError> {
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
            if branch.ended {
                log!(logger, "Skipping ended branch with {:?}", &predicate);
                blocked.insert(predicate, branch);
                continue;
            }
            use AssignmentUnionResult as Aur;
            log!(logger, "visiting branch with pred {:?}", &predicate);
            match predicate.assignment_union(&send_payload_msg.predicate) {
                Aur::Nonexistant => {
                    // this branch does not receive the message
                    log!(logger, "skipping branch");
                    blocked.insert(predicate, branch);
                }
                Aur::Equivalent | Aur::FormerNotLatter => {
                    // retain the existing predicate, but add this payload
                    log!(logger, "feeding this branch without altering its predicate");
                    branch.feed_msg(getter, send_payload_msg.payload.clone());
                    unblocked.insert(predicate, branch);
                }
                Aur::LatterNotFormer => {
                    // fork branch, give fork the message and payload predicate. original branch untouched
                    log!(logger, "Forking this branch, giving it the predicate of the msg");
                    let mut branch2 = branch.clone();
                    let predicate2 = send_payload_msg.predicate.clone();
                    branch2.feed_msg(getter, send_payload_msg.payload.clone());
                    blocked.insert(predicate, branch);
                    unblocked.insert(predicate2, branch2);
                }
                Aur::New(predicate2) => {
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
            rctx,
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
            if branch.ended && branch_predicate.assigns_subset(solution_predicate) {
                let ProtoComponentBranch { state, .. } = branch;
                return ProtoComponent { state, ports };
            }
        }
        panic!("ProtoComponent had no branches matching pred {:?}", solution_predicate);
    }
    fn initial(ProtoComponent { state, ports }: ProtoComponent) -> Self {
        let branch = ProtoComponentBranch {
            inbox: Default::default(),
            did_put_or_get: Default::default(),
            state,
            ended: false,
            untaken_choice: None,
        };
        Self { ports, branches: hashmap! { Predicate::default() => branch  } }
    }
}
impl SolutionStorage {
    fn new(subtree_ids: impl Iterator<Item = SubtreeId>) -> Self {
        let mut subtree_id_to_index: HashMap<SubtreeId, usize> = Default::default();
        let mut subtree_solutions = vec![];
        for id in subtree_ids {
            subtree_id_to_index.insert(id, subtree_solutions.len());
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
    fn reset(&mut self, subtree_ids: impl Iterator<Item = SubtreeId>) {
        self.subtree_id_to_index.clear();
        self.subtree_solutions.clear();
        self.old_local.clear();
        self.new_local.clear();
        for key in subtree_ids {
            self.subtree_id_to_index.insert(key, self.subtree_solutions.len());
            self.subtree_solutions.push(Default::default())
        }
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
        subtree_id: SubtreeId,
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
impl GetterBuffer {
    fn len(&self) -> usize {
        self.getters_and_sends.len()
    }
    fn pop(&mut self) -> Option<(PortId, SendPayloadMsg)> {
        self.getters_and_sends.pop()
    }
    fn getter_add(&mut self, getter: PortId, msg: SendPayloadMsg) {
        self.getters_and_sends.push((getter, msg));
    }
    fn putter_add(&mut self, cu: &mut ConnectorUnphased, putter: PortId, msg: SendPayloadMsg) {
        if let Some(&getter) = cu.port_info.peers.get(&putter) {
            self.getter_add(getter, msg);
        } else {
            log!(cu.logger, "Putter {:?} has no known peer!", putter);
            panic!("Putter {:?} has no known peer!");
        }
    }
}
impl SyncProtoContext<'_> {
    pub(crate) fn is_firing(&mut self, port: PortId) -> Option<bool> {
        let var = self.port_info.spec_var_for(port);
        self.predicate.query(var).map(SpecVal::is_firing)
    }
    pub(crate) fn read_msg(&mut self, port: PortId) -> Option<&Payload> {
        self.did_put_or_get.insert(port);
        self.inbox.get(&port)
    }
    pub(crate) fn take_choice(&mut self) -> Option<u16> {
        self.untaken_choice.take()
    }
}
impl<'a, K: Eq + Hash, V> CyclicDrainInner<'a, K, V> {
    fn add_input(&mut self, k: K, v: V) {
        self.swap.insert(k, v);
    }
    fn merge_input_with<F: FnMut(V, &mut V) -> V>(&mut self, k: K, v: V, mut func: F) {
        use std::collections::hash_map::Entry;
        let e = self.swap.entry(k);
        match e {
            Entry::Vacant(ev) => {
                ev.insert(v);
            }
            Entry::Occupied(mut eo) => {
                let old = eo.get_mut();
                *old = func(v, old);
            }
        }
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
    fn cyclic_drain<E>(
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
