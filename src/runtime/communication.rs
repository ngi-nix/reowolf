use super::*;
use crate::common::*;
use core::ops::{Deref, DerefMut};

////////////////
// Guard protecting an incrementally unfoldable slice of MapTempGuard elements
struct MapTempsGuard<'a, K, V>(&'a mut [HashMap<K, V>]);
// Type protecting a temporary map; At the start and end of the Guard's lifetime, self.0.is_empty() must be true
struct MapTempGuard<'a, K, V>(&'a mut HashMap<K, V>);

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
struct BranchingProtoComponent {
    branches: HashMap<Predicate, ProtoComponentBranch>,
}
#[derive(Debug, Clone)]
struct ProtoComponentBranch {
    state: ComponentState,
    inner: ProtoComponentBranchInner,
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
// CuUndecided provides a mostly immutable view into the ConnectorUnphased structure,
// making it harder to accidentally mutate its contents in a way that cannot be rolled back.
impl CuUndecided for ConnectorUnphased {
    fn logger_and_protocol_description(&mut self) -> (&mut dyn Logger, &ProtocolDescription) {
        (&mut *self.inner.logger, &self.proto_description)
    }
    fn logger(&mut self) -> &mut dyn Logger {
        &mut *self.inner.logger
    }
    fn proto_description(&self) -> &ProtocolDescription {
        &self.proto_description
    }
    fn native_component_id(&self) -> ComponentId {
        self.inner.native_component_id
    }
}

////////////////
impl<'a, K, V> MapTempsGuard<'a, K, V> {
    fn reborrow(&mut self) -> MapTempsGuard<'_, K, V> {
        MapTempsGuard(self.0)
    }
    fn split_first_mut(self) -> (MapTempGuard<'a, K, V>, MapTempsGuard<'a, K, V>) {
        let (head, tail) = self.0.split_first_mut().expect("Cache exhausted");
        (MapTempGuard::new(head), MapTempsGuard(tail))
    }
}
impl<'a, K, V> MapTempGuard<'a, K, V> {
    fn new(map: &'a mut HashMap<K, V>) -> Self {
        assert!(map.is_empty()); // sanity check
        Self(map)
    }
}
impl<'a, K, V> Drop for MapTempGuard<'a, K, V> {
    fn drop(&mut self) {
        assert!(self.0.is_empty()); // sanity check
    }
}
impl<'a, K, V> Deref for MapTempGuard<'a, K, V> {
    type Target = HashMap<K, V>;
    fn deref(&self) -> &<Self as Deref>::Target {
        self.0
    }
}
impl<'a, K, V> DerefMut for MapTempGuard<'a, K, V> {
    fn deref_mut(&mut self) -> &mut <Self as Deref>::Target {
        self.0
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
        let Self { unphased: cu, phased } = self;
        let info = cu.inner.current_state.port_info.get(&port).ok_or(Poe::UnknownPolarity)?;
        if info.owner != cu.inner.native_component_id {
            return Err(Poe::PortUnavailable);
        }
        if info.polarity != expect_polarity {
            return Err(Poe::WrongPolarity);
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
                        log!(cu.logger(), "Attempted to start sync round, but previous error {:?} was unrecoverable!", e);
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

        log!(@MARK, cu.logger(), "sync start {}", comm.round_index);
        log!(
            cu.logger(),
            "~~~ SYNC called with timeout {:?}; starting round {}",
            &timeout,
            comm.round_index
        );
        log!(@BENCH, cu.logger(), "");

        // 1. run all proto components to Nonsync blockers
        // iterate
        let mut current_state = cu.inner.current_state.clone();
        let mut branching_proto_components =
            HashMap::<ComponentId, BranchingProtoComponent>::default();
        let mut unrun_components: Vec<(ComponentId, ComponentState)> = cu
            .proto_components
            .iter()
            .map(|(&proto_id, proto)| (proto_id, proto.clone()))
            .collect();
        log!(cu.logger(), "Nonsync running {} proto components...", unrun_components.len());
        // drains unrun_components, and populates branching_proto_components.
        while let Some((proto_component_id, mut component)) = unrun_components.pop() {
            log!(
                cu.logger(),
                "Nonsync running proto component with ID {:?}. {} to go after this",
                proto_component_id,
                unrun_components.len()
            );
            let (logger, proto_description) = cu.logger_and_protocol_description();
            let mut ctx = NonsyncProtoContext {
                current_state: &mut current_state,
                logger,
                proto_component_id,
                unrun_components: &mut unrun_components,
            };
            let blocker = component.nonsync_run(&mut ctx, proto_description);
            log!(
                cu.logger(),
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
            cu.logger(),
            "All {} proto components are now done with Nonsync phase",
            branching_proto_components.len(),
        );
        log!(@BENCH, cu.logger(), "");

        // Create temp structures needed for the synchronous phase of the round
        let mut rctx = RoundCtx {
            current_state,
            solution_storage: {
                let n = std::iter::once(SubtreeId::LocalComponent(cu.inner.native_component_id));
                let c =
                    branching_proto_components.keys().map(|&cid| SubtreeId::LocalComponent(cid));
                let e = comm
                    .neighborhood
                    .children
                    .iter()
                    .map(|&index| SubtreeId::NetEndpoint { index });
                let subtree_id_iter = n.chain(c).chain(e);
                log!(
                    cu.inner.logger,
                    "Children in subtree are: {:?}",
                    subtree_id_iter.clone().collect::<Vec<_>>()
                );
                SolutionStorage::new(subtree_id_iter)
            },
            spec_var_stream: cu.inner.current_state.id_manager.new_spec_var_stream(),
            payload_inbox: Default::default(),
            deadline: timeout.map(|to| Instant::now() + to),
        };
        log!(cu.logger(), "Round context structure initialized");
        log!(@BENCH, cu.logger(), "");

        // Explore all native branches eagerly. Find solutions, buffer messages, etc.
        log!(
            cu.logger(),
            "Translating {} native batches into branches...",
            comm.native_batches.len()
        );
        let native_spec_var = rctx.spec_var_stream.next();
        log!(cu.logger(), "Native branch spec var is {:?}", native_spec_var);
        let mut branching_native = BranchingNative { branches: Default::default() };
        'native_branches: for ((native_branch, index), branch_spec_val) in
            comm.native_batches.drain(..).zip(0..).zip(SpecVal::iter_domain())
        {
            let NativeBatch { to_get, to_put } = native_branch;
            let predicate = {
                let mut predicate = Predicate::default();
                // all firing ports have SpecVal::FIRING
                let firing_iter = to_get.iter().chain(to_put.keys()).copied();

                log!(
                    cu.logger(),
                    "New native with firing ports {:?}",
                    firing_iter.clone().collect::<Vec<_>>()
                );
                let firing_ports: HashSet<PortId> = firing_iter.clone().collect();
                for port in firing_iter {
                    let var = cu.inner.current_state.spec_var_for(port);
                    predicate.assigned.insert(var, SpecVal::FIRING);
                }
                // all silent ports have SpecVal::SILENT
                for (port, port_info) in cu.inner.current_state.port_info.iter() {
                    if port_info.owner != cu.inner.native_component_id {
                        // not my port
                        continue;
                    }
                    if firing_ports.contains(port) {
                        // this one is FIRING
                        continue;
                    }
                    let var = cu.inner.current_state.spec_var_for(*port);
                    if let Some(SpecVal::FIRING) = predicate.assigned.insert(var, SpecVal::SILENT) {
                        log!(cu.logger(), "Native branch index={} contains internal inconsistency wrt. {:?}. Skipping", index, var);
                        continue 'native_branches;
                    }
                }
                // this branch is consistent. distinguish it with a unique var:val mapping and proceed
                predicate.inserted(native_spec_var, branch_spec_val)
            };
            log!(cu.logger(), "Native branch index={:?} has consistent {:?}", index, &predicate);
            // send all outgoing messages (by buffering them)
            for (putter, payload) in to_put {
                let msg = SendPayloadMsg { predicate: predicate.clone(), payload };
                log!(
                    cu.logger(),
                    "Native branch {} sending msg {:?} with putter {:?}",
                    index,
                    &msg,
                    putter
                );
                // sanity check
                assert_eq!(Putter, cu.inner.current_state.port_info.get(&putter).unwrap().polarity);
                rctx.putter_push(cu, putter, msg);
            }
            let branch = NativeBranch { index, gotten: Default::default(), to_get };
            if branch.is_ended() {
                log!(
                    cu.logger(),
                    "Native submitting solution for batch {} with {:?}",
                    index,
                    &predicate
                );
                rctx.solution_storage.submit_and_digest_subtree_solution(
                    cu,
                    SubtreeId::LocalComponent(cu.inner.native_component_id),
                    predicate.clone(),
                );
            }
            if let Some(_) = branching_native.branches.insert(predicate, branch) {
                // thanks to the native_spec_var, each batch has a distinct predicate
                unreachable!()
            }
        }
        // restore the invariant: !native_batches.is_empty()
        comm.native_batches.push(Default::default());
        // Call to another big method; keep running this round until a distributed decision is reached
        log!(cu.logger(), "Searching for decision...");
        log!(@BENCH, cu.logger(), "");
        let decision = Self::sync_reach_decision(
            cu,
            comm,
            &mut branching_native,
            &mut branching_proto_components,
            &mut rctx,
        )?;
        log!(@MARK, cu.logger(), "got decision!");
        log!(cu.logger(), "Committing to decision {:?}!", &decision);
        log!(@BENCH, cu.logger(), "");
        comm.endpoint_manager.udp_endpoints_round_end(&mut *cu.logger(), &decision)?;

        // propagate the decision to children
        let msg = Msg::CommMsg(CommMsg {
            round_index: comm.round_index,
            contents: CommMsgContents::CommCtrl(CommCtrlMsg::Announce {
                decision: decision.clone(),
            }),
        });
        log!(
            cu.logger(),
            "Announcing decision {:?} through child endpoints {:?}",
            &msg,
            &comm.neighborhood.children
        );
        log!(@MARK, cu.logger(), "forwarding decision!");
        for &child in comm.neighborhood.children.iter() {
            comm.endpoint_manager.send_to_comms(child, &msg)?;
        }
        let ret = match decision {
            Decision::Failure => {
                // dropping {branching_proto_components, branching_native}
                log!(cu.inner.logger, "Failure with {:#?}", &rctx.solution_storage);
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
                // commit changes to ports and id_manager
                cu.inner.current_state = rctx.current_state;
                log!(
                    cu.inner.logger,
                    "End round with (updated) component states {:?}",
                    cu.proto_components.keys()
                );
                // consume native
                Ok(Some(branching_native.collapse_with(&mut *cu.logger(), &predicate)))
            }
        };
        log!(cu.logger(), "Sync round ending! Cleaning up");
        log!(@BENCH, cu.logger(), "");
        ret
    }

    fn sync_reach_decision(
        cu: &mut impl CuUndecided,
        comm: &mut ConnectorCommunication,
        branching_native: &mut BranchingNative,
        branching_proto_components: &mut HashMap<ComponentId, BranchingProtoComponent>,
        rctx: &mut RoundCtx,
    ) -> Result<Decision, UnrecoverableSyncError> {
        log!(@MARK, cu.logger(), "decide start");
        let mut already_requested_failure = false;
        if branching_native.branches.is_empty() {
            log!(cu.logger(), "Native starts with no branches! Failure!");
            match comm.neighborhood.parent {
                Some(parent) => {
                    if already_requested_failure.replace_with_true() {
                        Self::request_failure(cu, comm, parent)?
                    } else {
                        log!(cu.logger(), "Already requested failure");
                    }
                }
                None => {
                    log!(cu.logger(), "No parent. Deciding on failure");
                    return Ok(Decision::Failure);
                }
            }
        }
        let mut pcb_temps_owner = <[HashMap<Predicate, ProtoComponentBranch>; 3]>::default();
        let mut pcb_temps = MapTempsGuard(&mut pcb_temps_owner);
        let mut bn_temp_owner = <HashMap<Predicate, NativeBranch>>::default();

        // run all proto components to their sync blocker
        log!(
            cu.logger(),
            "Running all {} proto components to their sync blocker...",
            branching_proto_components.len()
        );
        for (&proto_component_id, proto_component) in branching_proto_components.iter_mut() {
            let BranchingProtoComponent { branches } = proto_component;
            // must reborrow to constrain the lifetime of pcb_temps to inside the loop
            let (swap, pcb_temps) = pcb_temps.reborrow().split_first_mut();
            let (blocked, _pcb_temps) = pcb_temps.split_first_mut();
            // initially, no components have .ended==true
            // drain from branches --> blocked
            let cd = CyclicDrainer::new(branches, swap.0, blocked.0);
            BranchingProtoComponent::drain_branches_to_blocked(cd, cu, rctx, proto_component_id)?;
            // swap the blocked branches back
            std::mem::swap(blocked.0, branches);
            if branches.is_empty() {
                log!(cu.logger(), "{:?} has become inconsistent!", proto_component_id);
                if let Some(parent) = comm.neighborhood.parent {
                    if already_requested_failure.replace_with_true() {
                        Self::request_failure(cu, comm, parent)?
                    } else {
                        log!(cu.logger(), "Already requested failure");
                    }
                } else {
                    log!(cu.logger(), "As the leader, deciding on timeout");
                    return Ok(Decision::Failure);
                }
            }
        }
        log!(cu.logger(), "All proto components are blocked");

        log!(cu.logger(), "Entering decision loop...");
        comm.endpoint_manager.undelay_all();
        'undecided: loop {
            // drain payloads_to_get, sending them through endpoints / feeding them to components
            log!(cu.logger(), "Decision loop! have {} messages to recv", rctx.payload_inbox.len());
            while let Some((getter, send_payload_msg)) = rctx.getter_pop() {
                log!(@MARK, cu.logger(), "handling payload msg for getter {:?} of {:?}", getter, &send_payload_msg);
                let getter_info = rctx.current_state.port_info.get(&getter).unwrap();
                let cid = getter_info.owner;
                assert_eq!(Getter, getter_info.polarity);
                log!(
                    cu.logger(),
                    "Routing msg {:?} to {:?} via {:?}",
                    &send_payload_msg,
                    getter,
                    &getter_info.route
                );
                match getter_info.route {
                    Route::UdpEndpoint { index } => {
                        let udp_endpoint_ext =
                            &mut comm.endpoint_manager.udp_endpoint_store.endpoint_exts[index];
                        let SendPayloadMsg { predicate, payload } = send_payload_msg;
                        log!(cu.logger(), "Delivering to udp endpoint index={}", index);
                        udp_endpoint_ext.outgoing_payloads.insert(predicate, payload);
                    }
                    Route::NetEndpoint { index } => {
                        log!(@MARK, cu.logger(), "sending payload");
                        let msg = Msg::CommMsg(CommMsg {
                            round_index: comm.round_index,
                            contents: CommMsgContents::SendPayload(send_payload_msg),
                        });
                        comm.endpoint_manager.send_to_comms(index, &msg)?;
                    }
                    Route::LocalComponent if cid == cu.native_component_id() => branching_native
                        .feed_msg(
                            cu,
                            rctx,
                            getter,
                            &send_payload_msg,
                            MapTempGuard::new(&mut bn_temp_owner),
                        ),
                    Route::LocalComponent => {
                        if let Some(branching_component) = branching_proto_components.get_mut(&cid)
                        {
                            branching_component.feed_msg(
                                cu,
                                rctx,
                                cid,
                                getter,
                                &send_payload_msg,
                                pcb_temps.reborrow(),
                            )?;
                            if branching_component.branches.is_empty() {
                                log!(cu.logger(), "{:?} has become inconsistent!", cid);
                                if let Some(parent) = comm.neighborhood.parent {
                                    if already_requested_failure.replace_with_true() {
                                        Self::request_failure(cu, comm, parent)?
                                    } else {
                                        log!(cu.logger(), "Already requested failure");
                                    }
                                } else {
                                    log!(cu.logger(), "As the leader, deciding on timeout");
                                    return Ok(Decision::Failure);
                                }
                            }
                        } else {
                            log!(
                                cu.logger(),
                                "Delivery to getter {:?} msg {:?} failed because {:?} isn't here",
                                getter,
                                &send_payload_msg,
                                cid
                            );
                        }
                    }
                }
            }

            // check if we have a solution yet
            log!(cu.logger(), "Check if we have any local decisions...");
            for solution in rctx.solution_storage.iter_new_local_make_old() {
                log!(cu.logger(), "New local decision with solution {:?}...", &solution);
                log!(@MARK, cu.logger(), "local solution");
                match comm.neighborhood.parent {
                    Some(parent) => {
                        log!(cu.logger(), "Forwarding to my parent {:?}", parent);
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
                        log!(cu.logger(), "No parent. Deciding on solution {:?}", &solution);
                        return Ok(Decision::Success(solution));
                    }
                }
            }

            // stuck! make progress by receiving a msg
            // try recv messages arriving through endpoints
            log!(cu.logger(), "No decision yet. Let's recv an endpoint msg...");
            {
                let (net_index, comm_ctrl_msg): (usize, CommCtrlMsg) =
                    match comm.endpoint_manager.try_recv_any_comms(cu, rctx, comm.round_index)? {
                        CommRecvOk::NewControlMsg { net_index, msg } => (net_index, msg),
                        CommRecvOk::NewPayloadMsgs => continue 'undecided,
                        CommRecvOk::TimeoutWithoutNew => {
                            log!(cu.logger(), "Reached user-defined deadling without decision...");
                            if let Some(parent) = comm.neighborhood.parent {
                                if already_requested_failure.replace_with_true() {
                                    Self::request_failure(cu, comm, parent)?
                                } else {
                                    log!(cu.logger(), "Already requested failure");
                                }
                            } else {
                                log!(cu.logger(), "As the leader, deciding on timeout");
                                return Ok(Decision::Failure);
                            }
                            rctx.deadline = None;
                            continue 'undecided;
                        }
                    };
                log!(
                    cu.logger(),
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
                                    log!(cu.logger(), "Child provided solution {:?}", &predicate);
                                    let subtree_id = SubtreeId::NetEndpoint { index: net_index };
                                    rctx.solution_storage.submit_and_digest_subtree_solution(
                                        cu, subtree_id, predicate,
                                    );
                                }
                                Decision::Failure => {
                                    match comm.neighborhood.parent {
                                        None => {
                                            log!(cu.logger(), "I decide on my child's failure");
                                            break 'undecided Ok(Decision::Failure);
                                        }
                                        Some(parent) => {
                                            log!(cu.logger(), "Forwarding failure through my parent endpoint {:?}", parent);
                                            if already_requested_failure.replace_with_true() {
                                                Self::request_failure(cu, comm, parent)?
                                            } else {
                                                log!(cu.logger(), "Already requested failure");
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            log!(
                                cu.logger(),
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
                                cu.logger(),
                                "Discarding announcement {:?} from non-parent endpoint idx {:?}",
                                &decision,
                                net_index
                            );
                        }
                    }
                }
            }
            log!(cu.logger(), "Endpoint msg recv done");
        }
    }
    fn request_failure(
        cu: &mut impl CuUndecided,
        comm: &mut ConnectorCommunication,
        parent: usize,
    ) -> Result<(), UnrecoverableSyncError> {
        log!(cu.logger(), "Forwarding to my parent {:?}", parent);
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
        cu: &mut impl CuUndecided,
        rctx: &mut RoundCtx,
        getter: PortId,
        send_payload_msg: &SendPayloadMsg,
        bn_temp: MapTempGuard<'_, Predicate, NativeBranch>,
    ) {
        log!(cu.logger(), "feeding native getter {:?} {:?}", getter, &send_payload_msg);
        assert_eq!(Getter, rctx.current_state.port_info.get(&getter).unwrap().polarity);
        let mut draining = bn_temp;
        let finished = &mut self.branches;
        std::mem::swap(draining.0, finished);
        for (predicate, mut branch) in draining.drain() {
            log!(cu.logger(), "visiting native branch {:?} with {:?}", &branch, &predicate);
            // check if this branch expects to receive it
            let var = rctx.current_state.spec_var_for(getter);
            let mut feed_branch = |branch: &mut NativeBranch, predicate: &Predicate| {
                branch.to_get.remove(&getter);
                let was = branch.gotten.insert(getter, send_payload_msg.payload.clone());
                assert!(was.is_none());
                if branch.is_ended() {
                    log!(
                        cu.logger(),
                        "new native solution with {:?} is_ended() with gotten {:?}",
                        &predicate,
                        &branch.gotten
                    );
                    let subtree_id = SubtreeId::LocalComponent(cu.native_component_id());
                    rctx.solution_storage.submit_and_digest_subtree_solution(
                        cu,
                        subtree_id,
                        predicate.clone(),
                    );
                } else {
                    log!(
                        cu.logger(),
                        "Fed native {:?} still has to_get {:?}",
                        &predicate,
                        &branch.to_get
                    );
                }
            };
            if predicate.query(var) != Some(SpecVal::FIRING) {
                // optimization. Don't bother trying this branch
                log!(
                    cu.logger(),
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
                        cu.logger(),
                        "skipping branch with {:?} that doesn't want the message (slowpath)",
                        &predicate
                    );
                    Self::insert_branch_merging(finished, predicate, branch);
                }
                Aur::Equivalent | Aur::FormerNotLatter => {
                    // retain the existing predicate, but add this payload
                    feed_branch(&mut branch, &predicate);
                    log!(cu.logger(), "branch pred covers it! Accept the msg");
                    Self::insert_branch_merging(finished, predicate, branch);
                }
                Aur::LatterNotFormer => {
                    // fork branch, give fork the message and payload predicate. original branch untouched
                    let mut branch2 = branch.clone();
                    let predicate2 = send_payload_msg.predicate.clone();
                    feed_branch(&mut branch2, &predicate2);
                    log!(
                        cu.logger(),
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
                        cu.logger(),
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
        cu: &mut impl CuUndecided,
        rctx: &mut RoundCtx,
        proto_component_id: ComponentId,
    ) -> Result<(), UnrecoverableSyncError> {
        cd.cyclic_drain(|mut predicate, mut branch, mut drainer| {
            let mut ctx = SyncProtoContext {
                rctx,
                predicate: &predicate,
                branch_inner: &mut branch.inner,
            };
            let blocker = branch.state.sync_run(&mut ctx, cu.proto_description());
            log!(
                cu.logger(),
                "Proto component with id {:?} branch with pred {:?} hit blocker {:?}",
                proto_component_id,
                &predicate,
                &blocker,
            );
            use SyncBlocker as B;
            match blocker {
                B::Inconsistent => drop((predicate, branch)), // EXPLICIT inconsistency
                B::NondetChoice { n } => {
                    let var = rctx.spec_var_stream.next();
                    for val in SpecVal::iter_domain().take(n as usize) {
                        let pred = predicate.clone().inserted(var, val);
                        let mut branch_n = branch.clone();
                        branch_n.inner.untaken_choice = Some(val.0);
                        drainer.add_input(pred, branch_n);
                    }
                }
                B::CouldntReadMsg(port) => {
                    // move to "blocked"
                    assert!(!branch.inner.inbox.contains_key(&port));
                    drainer.add_output(predicate, branch);
                }
                B::CouldntCheckFiring(port) => {
                    // sanity check
                    let var = rctx.current_state.spec_var_for(port);
                    assert!(predicate.query(var).is_none());
                    // keep forks in "unblocked"
                    drainer.add_input(predicate.clone().inserted(var, SpecVal::SILENT), branch.clone());
                    drainer.add_input(predicate.inserted(var, SpecVal::FIRING), branch);
                }
                B::PutMsg(putter, payload) => {
                    // sanity check
                    assert_eq!(Putter, rctx.current_state.port_info.get(&putter).unwrap().polarity);
                    // overwrite assignment
                    let var = rctx.current_state.spec_var_for(putter);
                    let was = predicate.assigned.insert(var, SpecVal::FIRING);
                    if was == Some(SpecVal::SILENT) {
                        log!(cu.logger(), "Proto component {:?} tried to PUT on port {:?} when pred said var {:?}==Some(false). inconsistent!",
                            proto_component_id, putter, var);
                        // discard forever
                        drop((predicate, branch));
                    } else {
                        // keep in "unblocked"
                        branch.inner.did_put_or_get.insert(putter);
                        log!(cu.logger(), "Proto component {:?} with pred {:?} putting payload {:?} on port {:?} (using var {:?})",
                            proto_component_id, &predicate, &payload, putter, var);
                        let msg = SendPayloadMsg { predicate: predicate.clone(), payload };
                        rctx.putter_push(cu, putter, msg);
                        drainer.add_input(predicate, branch);
                    }
                }
                B::SyncBlockEnd => {
                    // make concrete all variables
                    for (port, port_info) in rctx.current_state.port_info.iter() {
                        if port_info.owner != proto_component_id {
                            continue;
                        }
                        let var = rctx.current_state.spec_var_for(*port);
                        let should_have_fired = branch.inner.did_put_or_get.contains(port);
                        let val = *predicate.assigned.entry(var).or_insert(SpecVal::SILENT);
                        let did_fire = val == SpecVal::FIRING;
                        if did_fire != should_have_fired {
                            log!(cu.logger(), "Inconsistent wrt. port {:?} var {:?} val {:?} did_fire={}, should_have_fired={}",
                                port, var, val, did_fire, should_have_fired);
                            // IMPLICIT inconsistency
                            drop((predicate, branch));
                            return Ok(());
                        }
                    }
                    // submit solution for this component
                    let subtree_id = SubtreeId::LocalComponent(proto_component_id);
                    rctx.solution_storage.submit_and_digest_subtree_solution(
                        cu,
                        subtree_id,
                        predicate.clone(),
                    );
                    branch.ended = true;
                    // move to "blocked"
                    drainer.add_output(predicate, branch);
                }
            }
            Ok(())
        })
    }
    fn feed_msg(
        &mut self,
        cu: &mut impl CuUndecided,
        rctx: &mut RoundCtx,
        proto_component_id: ComponentId,
        getter: PortId,
        send_payload_msg: &SendPayloadMsg,
        pcb_temps: MapTempsGuard<'_, Predicate, ProtoComponentBranch>,
    ) -> Result<(), UnrecoverableSyncError> {
        log!(
            cu.logger(),
            "feeding proto component {:?} getter {:?} {:?}",
            proto_component_id,
            getter,
            &send_payload_msg
        );
        let BranchingProtoComponent { branches } = self;
        let (mut unblocked, pcb_temps) = pcb_temps.split_first_mut();
        let (mut blocked, pcb_temps) = pcb_temps.split_first_mut();
        // partition drain from branches -> {unblocked, blocked}
        log!(cu.logger(), "visiting {} blocked branches...", branches.len());
        for (predicate, mut branch) in branches.drain() {
            if branch.ended {
                log!(cu.logger(), "Skipping ended branch with {:?}", &predicate);
                Self::insert_branch_merging(&mut blocked, predicate, branch);
                continue;
            }
            use AssignmentUnionResult as Aur;
            log!(cu.logger(), "visiting branch with pred {:?}", &predicate);
            match predicate.assignment_union(&send_payload_msg.predicate) {
                Aur::Nonexistant => {
                    // this branch does not receive the message
                    log!(cu.logger(), "skipping branch");
                    Self::insert_branch_merging(&mut blocked, predicate, branch);
                }
                Aur::Equivalent | Aur::FormerNotLatter => {
                    // retain the existing predicate, but add this payload
                    log!(cu.logger(), "feeding this branch without altering its predicate");
                    branch.feed_msg(getter, send_payload_msg.payload.clone());
                    Self::insert_branch_merging(&mut unblocked, predicate, branch);
                }
                Aur::LatterNotFormer => {
                    // fork branch, give fork the message and payload predicate. original branch untouched
                    log!(cu.logger(), "Forking this branch, giving it the predicate of the msg");
                    let mut branch2 = branch.clone();
                    let predicate2 = send_payload_msg.predicate.clone();
                    branch2.feed_msg(getter, send_payload_msg.payload.clone());
                    Self::insert_branch_merging(&mut blocked, predicate, branch);
                    Self::insert_branch_merging(&mut unblocked, predicate2, branch2);
                }
                Aur::New(predicate2) => {
                    // fork branch, give fork the message and the new predicate. original branch untouched
                    log!(cu.logger(), "Forking this branch with new predicate {:?}", &predicate2);
                    let mut branch2 = branch.clone();
                    branch2.feed_msg(getter, send_payload_msg.payload.clone());
                    Self::insert_branch_merging(&mut blocked, predicate, branch);
                    Self::insert_branch_merging(&mut unblocked, predicate2, branch2);
                }
            }
        }
        log!(cu.logger(), "blocked {:?} unblocked {:?}", blocked.len(), unblocked.len());
        // drain from unblocked --> blocked
        let (swap, _pcb_temps) = pcb_temps.split_first_mut();
        let cd = CyclicDrainer::new(unblocked.0, swap.0, blocked.0);
        BranchingProtoComponent::drain_branches_to_blocked(cd, cu, rctx, proto_component_id)?;
        // swap the blocked branches back
        std::mem::swap(blocked.0, branches);
        log!(cu.logger(), "component settles down with branches: {:?}", branches.keys());
        Ok(())
    }
    fn insert_branch_merging(
        branches: &mut HashMap<Predicate, ProtoComponentBranch>,
        predicate: Predicate,
        mut branch: ProtoComponentBranch,
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
                // This means keeping the existing one in-place, and giving it the UNION of the inboxes
                let old = eo.get_mut();
                for (k, v) in branch.inner.inbox.drain() {
                    old.inner.inbox.insert(k, v);
                }
                old.ended |= branch.ended;
            }
        }
    }
    fn collapse_with(self, solution_predicate: &Predicate) -> ComponentState {
        let BranchingProtoComponent { branches } = self;
        for (branch_predicate, branch) in branches {
            if branch.ended && branch_predicate.assigns_subset(solution_predicate) {
                let ProtoComponentBranch { state, .. } = branch;
                return state;
            }
        }
        panic!("ProtoComponent had no branches matching pred {:?}", solution_predicate);
    }
    fn initial(state: ComponentState) -> Self {
        let branch = ProtoComponentBranch { state, inner: Default::default(), ended: false };
        Self { branches: hashmap! { Predicate::default() => branch } }
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
    // fn is_clear(&self) -> bool {
    //     self.subtree_id_to_index.is_empty()
    //         && self.subtree_solutions.is_empty()
    //         && self.old_local.is_empty()
    //         && self.new_local.is_empty()
    // }
    // fn clear(&mut self) {
    //     self.subtree_id_to_index.clear();
    //     self.subtree_solutions.clear();
    //     self.old_local.clear();
    //     self.new_local.clear();
    // }
    // fn reset(&mut self, subtree_ids: impl Iterator<Item = SubtreeId>) {
    //     self.subtree_id_to_index.clear();
    //     self.subtree_solutions.clear();
    //     self.old_local.clear();
    //     self.new_local.clear();
    //     for key in subtree_ids {
    //         self.subtree_id_to_index.insert(key, self.subtree_solutions.len());
    //         self.subtree_solutions.push(Default::default())
    //     }
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
        cu: &mut impl CuUndecided,
        subtree_id: SubtreeId,
        predicate: Predicate,
    ) {
        log!(cu.logger(), "++ new component solution {:?} {:?}", subtree_id, &predicate);
        let index = self.subtree_id_to_index[&subtree_id];
        let left = 0..index;
        let right = (index + 1)..self.subtree_solutions.len();

        let Self { subtree_solutions, new_local, old_local, .. } = self;
        let was_new = subtree_solutions[index].insert(predicate.clone());
        if was_new {
            // iterator over SETS of solutions, one for every component except `subtree_id` (me)
            let set_visitor = left.chain(right).map(|index| &subtree_solutions[index]);
            Self::elaborate_into_new_local_rec(cu, predicate, set_visitor, old_local, new_local);
        }
    }
    fn elaborate_into_new_local_rec<'a, 'b>(
        cu: &mut impl CuUndecided,
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
                        cu,
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
                log!(cu.logger(), "storing NEW LOCAL SOLUTION {:?}", &partial);
                new_local.insert(partial);
            }
        }
    }
}
impl SyncProtoContext<'_> {
    pub(crate) fn is_firing(&mut self, port: PortId) -> Option<bool> {
        let var = self.rctx.current_state.spec_var_for(port);
        self.predicate.query(var).map(SpecVal::is_firing)
    }
    pub(crate) fn read_msg(&mut self, port: PortId) -> Option<&Payload> {
        self.branch_inner.did_put_or_get.insert(port);
        self.branch_inner.inbox.get(&port)
    }
    pub(crate) fn take_choice(&mut self) -> Option<u16> {
        self.branch_inner.untaken_choice.take()
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
        // sanity check
        for port in moved_ports.iter() {
            assert_eq!(
                self.proto_component_id,
                self.current_state.port_info.get(port).unwrap().owner
            );
        }
        // 2. create new component
        let new_cid = self.current_state.id_manager.new_component_id();
        log!(
            self.logger,
            "Component {:?} added new component {:?} with state {:?}, moving ports {:?}",
            self.proto_component_id,
            new_cid,
            &state,
            &moved_ports
        );
        self.unrun_components.push((new_cid, state));
        // 3. update ownership of moved ports
        for port in moved_ports.iter() {
            self.current_state.port_info.get_mut(port).unwrap().owner = new_cid;
        }
        // 3. create a new component
    }
    pub fn new_port_pair(&mut self) -> [PortId; 2] {
        // adds two new associated ports, related to each other, and exposed to the proto component
        let mut new_cid_fn = || self.current_state.id_manager.new_port_id();
        let [o, i] = [new_cid_fn(), new_cid_fn()];
        self.current_state.port_info.insert(
            o,
            PortInfo {
                route: Route::LocalComponent,
                peer: Some(i),
                polarity: Putter,
                owner: self.proto_component_id,
            },
        );
        self.current_state.port_info.insert(
            i,
            PortInfo {
                route: Route::LocalComponent,
                peer: Some(o),
                polarity: Getter,
                owner: self.proto_component_id,
            },
        );
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
        let e = self.inner.inbox.entry(getter);
        use std::collections::hash_map::Entry;
        match e {
            Entry::Vacant(ev) => {
                // new message
                ev.insert(payload);
            }
            Entry::Occupied(eo) => {
                // redundant recv. can happen as a result of a component A having two branches X and Y related by
                assert_eq!(eo.get(), &payload);
            }
        }
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
