use super::*;
use crate::common::*;
use core::ops::{Deref, DerefMut};

// Guard protecting an incrementally unfoldable slice of MapTempGuard elements
struct MapTempsGuard<'a, K, V>(&'a mut [HashMap<K, V>]);

// Type protecting a temporary map; At the start and end of the Guard's lifetime, self.0.is_empty() must be true
struct MapTempGuard<'a, K, V>(&'a mut HashMap<K, V>);

// Once the synchronous round has begun, this structure manages the
// native component's speculative branches, one per synchronous batch.
struct BranchingNative {
    branches: HashMap<Predicate, NativeBranch>,
}

// Corresponds to one of the native's synchronous batches during the synchronous round.
// ports marked for message receipt correspond to entries of
// (a) `gotten` if they have not received yet,
// (b) `to_get` if they have already received, with the given payload.
// The branch corresponds to a component solution IFF to_get is empty.
#[derive(Clone, Debug)]
struct NativeBranch {
    index: usize,
    gotten: HashMap<PortId, Payload>,
    to_get: HashSet<PortId>,
}

// Manages a protocol component's speculative branches for the duration
// of the synchronous round.
#[derive(Debug)]
struct BranchingProtoComponent {
    branches: HashMap<Predicate, ProtoComponentBranch>,
}

// One specualtive branch of a protocol component.
// `ended` IFF this branch has reached SyncBlocker::SyncBlockEnd before.
#[derive(Debug, Clone)]
struct ProtoComponentBranch {
    state: ComponentState,
    inner: ProtoComponentBranchInner,
    ended: bool,
}

// A structure wrapping a set of three pointers, making it impossible
// to miss that they are being setup for `cyclic_drain`.
struct CyclicDrainer<'a, K: Eq + Hash, V> {
    input: &'a mut HashMap<K, V>,
    swap: &'a mut HashMap<K, V>,
    output: &'a mut HashMap<K, V>,
}

// Small convenience trait for extending the stdlib's bool type with
// an optionlike replace method for increasing brevity.
trait ReplaceBoolTrue {
    fn replace_with_true(&mut self) -> bool;
}

//////////////// IMPL ////////////////////////////

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
        (&mut *self.logger, &self.proto_description)
    }
    fn logger_and_protocol_components(
        &mut self,
    ) -> (&mut dyn Logger, &mut HashMap<ComponentId, ComponentState>) {
        (&mut *self.logger, &mut self.proto_components)
    }
    fn logger(&mut self) -> &mut dyn Logger {
        &mut *self.logger
    }
    fn proto_description(&self) -> &ProtocolDescription {
        &self.proto_description
    }
    fn native_component_id(&self) -> ComponentId {
        self.native_component_id
    }
}
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
    /// Read the message received by the given port in the previous synchronous round.
    pub fn gotten(&self, port: PortId) -> Result<&Payload, GottenError> {
        use GottenError as Ge;
        if let ConnectorPhased::Communication(comm) = &self.phased {
            match &comm.round_result {
                Err(_) => Err(Ge::PreviousSyncFailed),
                Ok(None) => Err(Ge::NoPreviousRound),
                Ok(Some(round_ok)) => round_ok.gotten.get(&port).ok_or(Ge::PortDidntGet),
            }
        } else {
            return Err(Ge::NoPreviousRound);
        }
    }
    /// Creates a new, empty synchronous batch for the connector and selects it.
    /// Subsequent calls to `put` and `get` with populate the new batch with port operations.
    pub fn next_batch(&mut self) -> Result<usize, WrongStateError> {
        // returns index of new batch
        if let ConnectorPhased::Communication(comm) = &mut self.phased {
            comm.native_batches.push(Default::default());
            Ok(comm.native_batches.len() - 1)
        } else {
            Err(WrongStateError)
        }
    }

    fn port_op_access(
        &mut self,
        port: PortId,
        expect_polarity: Polarity,
    ) -> Result<&mut NativeBatch, PortOpError> {
        use PortOpError as Poe;
        let Self { unphased: cu, phased } = self;
        let info = cu.ips.port_info.map.get(&port).ok_or(Poe::UnknownPolarity)?;
        if info.owner != cu.native_component_id {
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

    /// Add a `put` operation to the connector's currently-selected synchronous batch.
    /// Returns an error if the given port is not owned by the native component,
    /// has the wrong polarity, or is already included in the batch.
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

    /// Add a `get` operation to the connector's currently-selected synchronous batch.
    /// Returns an error if the given port is not owned by the native component,
    /// has the wrong polarity, or is already included in the batch.
    pub fn get(&mut self, port: PortId) -> Result<(), PortOpError> {
        use PortOpError as Poe;
        let batch = self.port_op_access(port, Getter)?;
        if batch.to_get.insert(port) {
            Ok(())
        } else {
            Err(Poe::MultipleOpsOnPort)
        }
    }

    /// Participate in the completion of the next synchronous round, in which
    /// the native component will perform the set of prepared operations of exactly one
    /// of the synchronous batches. At the end of the procedure, the synchronous
    /// batches will be reset to a singleton set, whose only element is selected, and empty.
    /// The caller yields control over to the connector runtime to faciltiate the underlying
    /// coordination work until either (a) the round is completed with all components' states
    /// updated accordingly, (b) a distributed failure event resets all components'
    /// states to what they were prior to the sync call, or (c) the sync procedure encounters
    /// an unrecoverable error which ends the call early, and breaks the session and connector's
    /// states irreversably.
    /// Note that the (b) case necessitates the success of a distributed rollback procedure,
    /// which this component may initiate, but cannot guarantee will succeed in time or at all.
    /// consequently, the given timeout duration represents a duration in which the connector
    /// will make a best effort to fail the round and return control flow to the caller.
    pub fn sync(&mut self, timeout: Option<Duration>) -> Result<usize, SyncError> {
        // This method first destructures the connector, and checks for obvious
        // failure cases. The bulk of the behavior continues in `connected_sync`,
        // to minimize indentation, and enable convient ?-style short circuit syntax.
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

    // Attempts to complete the synchronous round for the given
    // communication-phased connector structure.
    // Modifies components and ports in `cu` IFF the round succeeds.
    #[inline]
    fn connected_sync(
        cu: &mut ConnectorUnphased,
        comm: &mut ConnectorCommunication,
        timeout: Option<Duration>,
    ) -> Result<Option<RoundEndedNative>, SyncError> {
        //////////////////////////////////
        use SyncError as Se;
        //////////////////////////////////

        // Create separate storages for ports and components stored in `cu`,
        // while kicking off the branching of components until the set of
        // components entering their synchronous block is finalized in `branching_proto_components`.
        // This is the last time cu's components and ports are accessed until the round is decided.
        let mut ips = cu.ips.clone();
        let mut branching_proto_components =
            HashMap::<ComponentId, BranchingProtoComponent>::default();
        let mut unrun_components: Vec<(ComponentId, ComponentState)> = cu
            .proto_components
            .iter()
            .map(|(&proto_id, proto)| (proto_id, proto.clone()))
            .collect();
        log!(cu.logger(), "Nonsync running {} proto components...", unrun_components.len());
        // initially, the set of components to run is the set of components stored by `cu`,
        // but they are eventually drained into `branching_proto_components`.
        // Some components exit first, and others are created and put into `unrun_components`.
        while let Some((proto_component_id, mut component)) = unrun_components.pop() {
            log!(
                cu.logger(),
                "Nonsync running proto component with ID {:?}. {} to go after this",
                proto_component_id,
                unrun_components.len()
            );
            let (logger, proto_description) = cu.logger_and_protocol_description();
            let mut ctx = NonsyncProtoContext {
                ips: &mut ips,
                logger,
                proto_component_id,
                unrun_components: &mut unrun_components,
            };
            let blocker = component.nonsync_run(&mut ctx, proto_description);
            log!(
                logger,
                "proto component {:?} ran to nonsync blocker {:?}",
                proto_component_id,
                &blocker
            );
            use NonsyncBlocker as B;
            match blocker {
                B::ComponentExit => drop(component),
                B::Inconsistent => return Err(Se::InconsistentProtoComponent(proto_component_id)),
                B::SyncBlockStart => assert!(branching_proto_components
                    .insert(proto_component_id, BranchingProtoComponent::initial(component))
                    .is_none()), // Some(_) returned IFF some component identifier key is overwritten (BAD!)
            }
        }
        log!(
            cu.logger(),
            "All {} proto components are now done with Nonsync phase",
            branching_proto_components.len(),
        );

        // Create temporary structures needed for the synchronous phase of the round
        let mut rctx = RoundCtx {
            ips, // already used previously, now moved into RoundCtx
            solution_storage: {
                let subtree_id_iter = {
                    // Create an iterator over the identifiers of this
                    // connector's childen in the _solution tree_.
                    // Namely, the native, all locally-managed components,
                    // and all this connector's children in the _consensus tree_ (other connectors).
                    let n = std::iter::once(SubtreeId::LocalComponent(cu.native_component_id));
                    let c = branching_proto_components
                        .keys()
                        .map(|&cid| SubtreeId::LocalComponent(cid));
                    let e = comm
                        .neighborhood
                        .children
                        .iter()
                        .map(|&index| SubtreeId::NetEndpoint { index });
                    n.chain(c).chain(e)
                };
                log!(
                    cu.logger,
                    "Children in subtree are: {:?}",
                    DebuggableIter(subtree_id_iter.clone())
                );
                SolutionStorage::new(subtree_id_iter)
            },
            spec_var_stream: cu.ips.id_manager.new_spec_var_stream(),
            payload_inbox: Default::default(), // buffer for in-memory payloads to be handled
            deadline: timeout.map(|to| Instant::now() + to),
        };
        log!(cu.logger(), "Round context structure initialized");

        // Prepare the branching native component, involving the conversion
        // of its synchronous batches (user provided) into speculative branches eagerly.
        // As a side effect, send all PUTs with the appropriate predicates.
        // Afterwards, each native component's speculative branch finds a local
        // solution the moment it's received all the messages it's awaiting.
        log!(
            cu.logger(),
            "Translating {} native batches into branches...",
            comm.native_batches.len()
        );
        // Allocate a single speculative variable to distinguish each native branch.
        // This enables native components to have distinct branches with identical
        // FIRING variables.
        let native_spec_var = rctx.spec_var_stream.next();
        log!(cu.logger(), "Native branch spec var is {:?}", native_spec_var);
        let mut branching_native = BranchingNative { branches: Default::default() };
        'native_branches: for ((native_branch, index), branch_spec_val) in
            comm.native_batches.drain(..).zip(0..).zip(SpecVal::iter_domain())
        {
            let NativeBatch { to_get, to_put } = native_branch;
            // compute the solution predicate to associate with this branch.
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
                    let var = cu.ips.port_info.spec_var_for(port);
                    predicate.assigned.insert(var, SpecVal::FIRING);
                }
                // all silent ports have SpecVal::SILENT
                for port in cu.ips.port_info.ports_owned_by(cu.native_component_id) {
                    if firing_ports.contains(port) {
                        // this one is FIRING
                        continue;
                    }
                    let var = cu.ips.port_info.spec_var_for(*port);
                    if let Some(SpecVal::FIRING) = predicate.assigned.insert(var, SpecVal::SILENT) {
                        log!(&mut *cu.logger, "Native branch index={} contains internal inconsistency wrt. {:?}. Skipping", index, var);
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
                assert_eq!(Putter, cu.ips.port_info.map.get(&putter).unwrap().polarity);
                rctx.putter_push(cu, putter, msg);
            }
            let branch = NativeBranch { index, gotten: Default::default(), to_get };
            if branch.is_ended() {
                // empty to_get set => already corresponds with a component solution
                log!(
                    cu.logger(),
                    "Native submitting solution for batch {} with {:?}",
                    index,
                    &predicate
                );
                rctx.solution_storage.submit_and_digest_subtree_solution(
                    cu,
                    SubtreeId::LocalComponent(cu.native_component_id),
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
        // Call to another big method; keep running this round
        // until a distributed decision is reached!
        log!(cu.logger(), "Searching for decision...");
        let decision = Self::sync_reach_decision(
            cu,
            comm,
            &mut branching_native,
            &mut branching_proto_components,
            &mut rctx,
        )?;
        log!(cu.logger(), "Committing to decision {:?}!", &decision);
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
        for &child in comm.neighborhood.children.iter() {
            comm.endpoint_manager.send_to_comms(child, &msg)?;
        }
        let ret = match decision {
            Decision::Failure => {
                // untouched port/component fields of `cu` are NOT overwritten.
                // the result is a rollback.
                Err(Se::RoundFailure)
            }
            Decision::Success(predicate) => {
                // commit changes to component states
                cu.proto_components.clear();
                let (logger, proto_components) = cu.logger_and_protocol_components();
                proto_components.extend(
                    // "flatten" branching components, committing the speculation
                    // consistent with the predicate decided upon.
                    branching_proto_components
                        .into_iter()
                        .map(|(cid, bpc)| (cid, bpc.collapse_with(logger, &predicate))),
                );
                // commit changes to ports and id_manager
                log!(
                    logger,
                    "End round with (updated) component states {:?}",
                    proto_components.keys()
                );
                cu.ips = rctx.ips;
                // consume native
                let round_ok = branching_native.collapse_with(cu.logger(), &predicate);
                Ok(Some(round_ok))
            }
        };
        log!(cu.logger(), "Sync round ending! Cleaning up");
        ret
    }

    // Once the synchronous round has been started, this procedure
    // routs and handles payloads, receives control messages from neighboring connectors,
    // checks for timeout, and aggregates solutions until a distributed decision is reached.
    // The decision is either a solution (success case), or a distributed timeout rollback (failure case)
    // The final possible outcome is an unrecoverable error, which results from some fundamental misbehavior,
    // a network channel breaking, etc.
    fn sync_reach_decision(
        cu: &mut impl CuUndecided,
        comm: &mut ConnectorCommunication,
        branching_native: &mut BranchingNative,
        branching_proto_components: &mut HashMap<ComponentId, BranchingProtoComponent>,
        rctx: &mut RoundCtx,
    ) -> Result<Decision, UnrecoverableSyncError> {
        // The round is in progress, and now its just a matter of arriving at a decision.
        let mut already_requested_failure = false;
        if branching_native.branches.is_empty() {
            // An unsatisfiable native is the easiest way to detect failure
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

        // Create a small set of "workspace" hashmaps, to be passed by-reference into various calls.
        // This is an optimization, avoiding repeated allocation.
        let mut pcb_temps_owner = <[HashMap<Predicate, ProtoComponentBranch>; 3]>::default();
        let mut pcb_temps = MapTempsGuard(&mut pcb_temps_owner);
        let mut bn_temp_owner = <HashMap<Predicate, NativeBranch>>::default();

        // first, we run every protocol component to their sync blocker.
        // Afterwards we establish a loop invariant: no new decision can be reached
        // without handling messages in the buffer or arriving from the network
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
            // initially, no protocol components have .ended==true
            // drain from branches --> blocked
            let cd = CyclicDrainer { input: branches, swap: swap.0, output: blocked.0 };
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
        // ...invariant established!

        log!(cu.logger(), "Entering decision loop...");
        comm.endpoint_manager.undelay_all();
        'undecided: loop {
            // handle all buffered messages, sending them through endpoints / feeding them to components
            log!(cu.logger(), "Decision loop! have {} messages to recv", rctx.payload_inbox.len());
            while let Some((getter, send_payload_msg)) = rctx.getter_pop() {
                let getter_info = rctx.ips.port_info.map.get(&getter).unwrap();
                let cid = getter_info.owner; // the id of the component owning `getter` port
                assert_eq!(Getter, getter_info.polarity); // sanity check
                log!(
                    cu.logger(),
                    "Routing msg {:?} to {:?} via {:?}",
                    &send_payload_msg,
                    getter,
                    &getter_info.route
                );
                match getter_info.route {
                    Route::UdpEndpoint { index } => {
                        // this is a message sent over the network through a UDP endpoint
                        let udp_endpoint_ext =
                            &mut comm.endpoint_manager.udp_endpoint_store.endpoint_exts[index];
                        let SendPayloadMsg { predicate, payload } = send_payload_msg;
                        log!(cu.logger(), "Delivering to udp endpoint index={}", index);
                        // UDP mediator messages are buffered until the end of the round,
                        // because they are still speculative
                        udp_endpoint_ext.outgoing_payloads.insert(predicate, payload);
                    }
                    Route::NetEndpoint { index } => {
                        // this is a message sent over the network as a control message
                        let msg = Msg::CommMsg(CommMsg {
                            round_index: comm.round_index,
                            contents: CommMsgContents::SendPayload(send_payload_msg),
                        });
                        // actually send the message now
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
                        // some other component_id routed locally. must be a protocol component!
                        if let Some(branching_component) = branching_proto_components.get_mut(&cid)
                        {
                            // The recipient component is still running!
                            // Feed it this message AND run it again until all branches are blocked
                            branching_component.feed_msg(
                                cu,
                                rctx,
                                cid,
                                getter,
                                &send_payload_msg,
                                pcb_temps.reborrow(),
                            )?;
                            if branching_component.branches.is_empty() {
                                // A solution is impossible! this component has zero branches
                                // Initiate a rollback
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
                            // This case occurs when the component owning `getter` has exited,
                            // but the putter is still running (and sent this message).
                            // we drop the message on the floor, because it cannot be involved
                            // in a solution (requires sending a message over a dead channel!).
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
            // payload buffer is empty.
            // check if we have a solution yet
            log!(cu.logger(), "Check if we have any local decisions...");
            for solution in rctx.solution_storage.iter_new_local_make_old() {
                log!(cu.logger(), "New local decision with solution {:?}...", &solution);
                match comm.neighborhood.parent {
                    Some(parent) => {
                        // Always forward connector-local solutions to my parent
                        // AS they are moved from new->old in solution storage.
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
            // try recv ONE message arriving through an endpoint
            log!(cu.logger(), "No decision yet. Let's recv an endpoint msg...");
            {
                // This is the first call that may block the thread!
                // Until a message arrives over the network, no new solutions are possible.
                let (net_index, comm_ctrl_msg): (usize, CommCtrlMsg) =
                    match comm.endpoint_manager.try_recv_any_comms(cu, rctx, comm.round_index)? {
                        CommRecvOk::NewControlMsg { net_index, msg } => (net_index, msg),
                        CommRecvOk::NewPayloadMsgs => {
                            // 1+ speculative payloads have been buffered
                            // but no other control messages that require further handling
                            // restart the loop to process the messages before blocking
                            continue 'undecided;
                        }
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
                            // disable future timeout events! our request for failure has been sent
                            // all we can do at this point is wait.
                            rctx.deadline = None;
                            continue 'undecided;
                        }
                    };
                // We received a control message that requires further action
                log!(
                    cu.logger(),
                    "Received from endpoint {} ctrl msg {:?}",
                    net_index,
                    &comm_ctrl_msg
                );
                match comm_ctrl_msg {
                    CommCtrlMsg::Suggest { suggestion } => {
                        // We receive the solution of another connector (part of the decision process)
                        // (only accept this through a child endpoint)
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
                                    // Someone timed out! propagate this to parent or decide
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
                            // Unreachable if all connectors are playing by the rules.
                            // Silently ignored instead of causing panic to make the
                            // runtime more robust against network fuzz
                            log!(
                                cu.logger(),
                                "Discarding suggestion {:?} from non-child endpoint idx {:?}",
                                &suggestion,
                                net_index
                            );
                        }
                    }
                    CommCtrlMsg::Announce { decision } => {
                        // Apparently this round is over! A decision has been reached
                        if Some(net_index) == comm.neighborhood.parent {
                            // We accept the decision because it comes from our parent.
                            // end this loop, and and the synchronous round
                            return Ok(decision);
                        } else {
                            // Again, unreachable if all connectors are playing by the rules
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

    // Send a failure request to my parent in the consensus tree
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
    // Feed the given payload to the native component
    // May result in discovering new component solutions,
    // or fork speculative branches if the message's predicate
    // is MORE SPECIFIC than the branches of the native
    fn feed_msg(
        &mut self,
        cu: &mut impl CuUndecided,
        rctx: &mut RoundCtx,
        getter: PortId,
        send_payload_msg: &SendPayloadMsg,
        bn_temp: MapTempGuard<'_, Predicate, NativeBranch>,
    ) {
        log!(cu.logger(), "feeding native getter {:?} {:?}", getter, &send_payload_msg);
        assert_eq!(Getter, rctx.ips.port_info.map.get(&getter).unwrap().polarity);
        let mut draining = bn_temp;
        let finished = &mut self.branches;
        std::mem::swap(draining.0, finished);
        // Visit all native's branches, and feed those whose current predicates are
        // consistent with that of the received message.
        for (predicate, mut branch) in draining.drain() {
            log!(cu.logger(), "visiting native branch {:?} with {:?}", &branch, &predicate);
            let var = rctx.ips.port_info.spec_var_for(getter);
            if predicate.query(var) != Some(SpecVal::FIRING) {
                // optimization. Don't bother trying this branch,
                // because the resulting branch would have an inconsistent predicate.
                // the existing branch asserts the getter port is SILENT
                log!(
                    cu.logger(),
                    "skipping branch with {:?} that doesn't want the message (fastpath)",
                    &predicate
                );
                Self::insert_branch_merging(finished, predicate, branch);
                continue;
            }
            // Define a little helper closure over `rctx`
            // for feeding the given branch this new payload,
            // and submitting any resulting solutions
            let mut feed_branch = |branch: &mut NativeBranch, predicate: &Predicate| {
                // This branch notes the getter port as "gotten"
                branch.to_get.remove(&getter);
                if let Some(was) = branch.gotten.insert(getter, send_payload_msg.payload.clone()) {
                    // Sanity check. Payload mapping (Predicate,Port) should be unique each round
                    assert_eq!(&was, &send_payload_msg.payload);
                }
                if branch.is_ended() {
                    // That was the last message the branch was awaiting!
                    // Submitting new component solution.
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
                    // This branch still has ports awaiting their messages
                    log!(
                        cu.logger(),
                        "Fed native {:?} still has to_get {:?}",
                        &predicate,
                        &branch.to_get
                    );
                }
            };
            use AssignmentUnionResult as Aur;
            match predicate.assignment_union(&send_payload_msg.predicate) {
                Aur::Nonexistant => {
                    // The predicates of this branch and the payload are incompatible
                    // retain this branch as-is
                    log!(
                        cu.logger(),
                        "skipping branch with {:?} that doesn't want the message (slowpath)",
                        &predicate
                    );
                    Self::insert_branch_merging(finished, predicate, branch);
                }
                Aur::Equivalent | Aur::FormerNotLatter => {
                    // The branch's existing predicate "covers" (is at least as specific)
                    // as that of the payload. Can feed this branch the message without altering
                    // the branch predicate.
                    feed_branch(&mut branch, &predicate);
                    log!(cu.logger(), "branch pred covers it! Accept the msg");
                    Self::insert_branch_merging(finished, predicate, branch);
                }
                Aur::LatterNotFormer => {
                    // The predicates of branch and payload are compatible,
                    // but that of the payload is strictly more specific than that of the latter.
                    // FORK the branch, feed the fork the message, and give it the payload's predicate.
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
                    // The predicates of branch and payload are compatible,
                    // but their union is some new predicate (both preds assign something new).
                    // FORK the branch, feed the fork the message, and give it the new predicate.
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

    // Insert a new speculate branch into the given storage,
    // MERGING it with an existing branch if their predicate keys clash.
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

    // Given the predicate for the round's solution, collapse this
    // branching native to an ended branch whose predicate is consistent with it.
    // return as `RoundEndedNative` the result of a native completing successful round
    fn collapse_with(
        self,
        logger: &mut dyn Logger,
        solution_predicate: &Predicate,
    ) -> RoundEndedNative {
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
                return RoundEndedNative { batch_index: index, gotten };
            }
        }
        log!(logger, "Native had no branches matching pred {:?}", solution_predicate);
        panic!("Native had no branches matching pred {:?}", solution_predicate);
    }
}
impl BranchingProtoComponent {
    // Create a singleton-branch branching protocol component as
    // speculation begins, with the given protocol state.
    fn initial(state: ComponentState) -> Self {
        let branch = ProtoComponentBranch { state, inner: Default::default(), ended: false };
        Self { branches: hashmap! { Predicate::default() => branch } }
    }

    // run all the given branches (cd.input) to their SyncBlocker,
    // populating cd.output by cyclically draining "input" -> "cd."input" / cd.output.
    // (to prevent concurrent r/w of one structure, we realize "input" as cd.input for reading and cd.swap for writing)
    // This procedure might lose branches, and it might create new branches.
    fn drain_branches_to_blocked(
        cd: CyclicDrainer<Predicate, ProtoComponentBranch>,
        cu: &mut impl CuUndecided,
        rctx: &mut RoundCtx,
        proto_component_id: ComponentId,
    ) -> Result<(), UnrecoverableSyncError> {
        // let CyclicDrainer { input, swap, output } = cd;
        while !cd.input.is_empty() {
            'branch_iter: for (mut predicate, mut branch) in cd.input.drain() {
                let mut ctx = SyncProtoContext {
                    rctx,
                    predicate: &predicate,
                    branch_inner: &mut branch.inner,
                };
                // Run this component's state to the next syncblocker for handling
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
                    B::CouldntReadMsg(port) => {
                        // sanity check: `CouldntReadMsg` returned IFF the message is unavailable
                        assert!(!branch.inner.inbox.contains_key(&port));
                        // This branch hit a proper blocker: progress awaits the receipt of some message. Exit the cycle.
                        Self::insert_branch_merging(cd.output, predicate, branch);
                    }
                    B::CouldntCheckFiring(port) => {
                        // sanity check: `CouldntCheckFiring` returned IFF the variable is speculatively assigned
                        let var = rctx.ips.port_info.spec_var_for(port);
                        assert!(predicate.query(var).is_none());
                        // speculate on the two possible values of `var`. Schedule both branches to be rerun.

                        Self::insert_branch_merging(
                            cd.swap,
                            predicate.clone().inserted(var, SpecVal::SILENT),
                            branch.clone(),
                        );
                        Self::insert_branch_merging(
                            cd.swap,
                            predicate.inserted(var, SpecVal::FIRING),
                            branch,
                        );
                    }
                    B::PutMsg(putter, payload) => {
                        // sanity check: The given port indeed has `Putter` polarity
                        assert_eq!(Putter, rctx.ips.port_info.map.get(&putter).unwrap().polarity);
                        // assign FIRING to this port's associated firing variable
                        let var = rctx.ips.port_info.spec_var_for(putter);
                        let was = predicate.assigned.insert(var, SpecVal::FIRING);
                        if was == Some(SpecVal::SILENT) {
                            // Discard the branch, as it clearly has contradictory requirements for this value.
                            log!(cu.logger(), "Proto component {:?} tried to PUT on port {:?} when pred said var {:?}==Some(false). inconsistent!",
                            proto_component_id, putter, var);
                            drop((predicate, branch));
                        } else {
                            // Note that this port has put this round,
                            // and assert that this isn't its 2nd time putting this round (otheriwse PDL programming error)
                            assert!(branch.inner.did_put_or_get.insert(putter));
                            log!(cu.logger(), "Proto component {:?} with pred {:?} putting payload {:?} on port {:?} (using var {:?})",
                            proto_component_id, &predicate, &payload, putter, var);
                            // Send the given payload (by buffering it).
                            let msg = SendPayloadMsg { predicate: predicate.clone(), payload };
                            rctx.putter_push(cu, putter, msg);
                            // Branch can still make progress. Schedule to be rerun

                            Self::insert_branch_merging(cd.swap, predicate, branch);
                        }
                    }
                    B::SyncBlockEnd => {
                        // This branch reached the end of it's synchronous block
                        // assign all variables of owned ports that DIDN'T fire to SILENT
                        for port in rctx.ips.port_info.ports_owned_by(proto_component_id) {
                            let var = rctx.ips.port_info.spec_var_for(*port);
                            let actually_exchanged = branch.inner.did_put_or_get.contains(port);
                            let val = *predicate.assigned.entry(var).or_insert(SpecVal::SILENT);
                            let speculated_to_fire = val == SpecVal::FIRING;
                            if actually_exchanged != speculated_to_fire {
                                log!(cu.logger(), "Inconsistent wrt. port {:?} var {:?} val {:?} actually_exchanged={}, speculated_to_fire={}",
                                port, var, val, actually_exchanged, speculated_to_fire);
                                // IMPLICIT inconsistency
                                drop((predicate, branch));
                                continue 'branch_iter;
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
                        // This branch exits the cyclic drain
                        Self::insert_branch_merging(cd.output, predicate, branch);
                    }
                }
            }
            std::mem::swap(cd.input, cd.swap);
        }
        Ok(())
    }

    // Feed this branching protocol component the given message, and
    // then run all branches until they are once again blocked.
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
        let (mut unblocked, pcb_temps) = pcb_temps.split_first_mut();
        let (mut blocked, pcb_temps) = pcb_temps.split_first_mut();
        // partition drain from self.branches -> {unblocked, blocked} (not cyclic)
        log!(cu.logger(), "visiting {} blocked branches...", self.branches.len());
        for (predicate, mut branch) in self.branches.drain() {
            if branch.ended {
                log!(cu.logger(), "Skipping ended branch with {:?}", &predicate);
                Self::insert_branch_merging(&mut blocked, predicate, branch);
                continue;
            }
            use AssignmentUnionResult as Aur;
            log!(cu.logger(), "visiting branch with pred {:?}", &predicate);
            // We give each branch a chance to receive this message,
            // those that do are maybe UNBLOCKED, and all others remain BLOCKED.
            match predicate.assignment_union(&send_payload_msg.predicate) {
                Aur::Nonexistant => {
                    // this branch does not receive the message. categorize into blocked.
                    log!(cu.logger(), "skipping branch");
                    Self::insert_branch_merging(&mut blocked, predicate, branch);
                }
                Aur::Equivalent | Aur::FormerNotLatter => {
                    // retain the existing predicate, but add this payload
                    log!(cu.logger(), "feeding this branch without altering its predicate");
                    branch.feed_msg(getter, send_payload_msg.payload.clone());
                    // this branch does receive the message. categorize into unblocked.
                    Self::insert_branch_merging(&mut unblocked, predicate, branch);
                }
                Aur::LatterNotFormer => {
                    // fork branch, give fork the message and payload predicate. original branch untouched
                    log!(cu.logger(), "Forking this branch, giving it the predicate of the msg");
                    let mut branch2 = branch.clone();
                    let predicate2 = send_payload_msg.predicate.clone();
                    branch2.feed_msg(getter, send_payload_msg.payload.clone());
                    // the branch that receives the message is unblocked, the original one is blocked
                    Self::insert_branch_merging(&mut blocked, predicate, branch);
                    Self::insert_branch_merging(&mut unblocked, predicate2, branch2);
                }
                Aur::New(predicate2) => {
                    // fork branch, give fork the message and the new predicate. original branch untouched
                    log!(cu.logger(), "Forking this branch with new predicate {:?}", &predicate2);
                    let mut branch2 = branch.clone();
                    branch2.feed_msg(getter, send_payload_msg.payload.clone());
                    // the branch that receives the message is unblocked, the original one is blocked
                    Self::insert_branch_merging(&mut blocked, predicate, branch);
                    Self::insert_branch_merging(&mut unblocked, predicate2, branch2);
                }
            }
        }
        log!(cu.logger(), "blocked {:?} unblocked {:?}", blocked.len(), unblocked.len());
        // drain from unblocked --> blocked
        let (swap, _pcb_temps) = pcb_temps.split_first_mut(); // peel off ONE temp storage map
        let cd = CyclicDrainer { input: unblocked.0, swap: swap.0, output: blocked.0 };
        BranchingProtoComponent::drain_branches_to_blocked(cd, cu, rctx, proto_component_id)?;
        // swap the blocked branches back
        std::mem::swap(blocked.0, &mut self.branches);
        log!(cu.logger(), "component settles down with branches: {:?}", self.branches.keys());
        Ok(())
    }

    // Insert a new speculate branch into the given storage,
    // MERGING it with an existing branch if their predicate keys clash.
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
            }
        }
    }

    // Given the predicate for the round's solution, collapse this
    // branching native to an ended branch whose predicate is consistent with it.
    fn collapse_with(
        self,
        logger: &mut dyn Logger,
        solution_predicate: &Predicate,
    ) -> ComponentState {
        let BranchingProtoComponent { branches } = self;
        for (branch_predicate, branch) in branches {
            if branch.ended && branch_predicate.assigns_subset(solution_predicate) {
                let ProtoComponentBranch { state, .. } = branch;
                return state;
            }
        }
        log!(logger, "ProtoComponent had no branches matching pred {:?}", solution_predicate);
        panic!("ProtoComponent had no branches matching pred {:?}", solution_predicate);
    }
}
impl ProtoComponentBranch {
    // Feed this branch received message.
    // It's safe to receive the same message repeatedly,
    // but if we receive a message with different contents,
    // it's a sign something has gone wrong! keys of type (port, round, predicate)
    // should always map to at most one message value!
    fn feed_msg(&mut self, getter: PortId, payload: Payload) {
        let e = self.inner.inbox.entry(getter);
        use std::collections::hash_map::Entry;
        match e {
            Entry::Vacant(ev) => {
                // new message
                ev.insert(payload);
            }
            Entry::Occupied(eo) => {
                // redundant recv. can happen as a result of a
                // component A having two branches X and Y related by
                assert_eq!(eo.get(), &payload);
            }
        }
    }
}
impl SolutionStorage {
    // Create a new solution storage, to manage the local solutions for
    // this connector and all of it's children (subtrees) in the solution tree.
    fn new(subtree_ids: impl Iterator<Item = SubtreeId>) -> Self {
        // For easy iteration, we store this SubtreeId => {Predicate}
        // structure instead as a pair of structures: a vector of predicate sets,
        // and a subtree_id-to-index lookup map
        let mut subtree_id_to_index: HashMap<SubtreeId, usize> = Default::default();
        let mut subtree_solutions = vec![];
        for id in subtree_ids {
            subtree_id_to_index.insert(id, subtree_solutions.len());
            subtree_solutions.push(Default::default())
        }
        // new_local U old_local represents the solutions of this connector itself:
        // namely, those that can be created from the union of one element from each child's solution set.
        // The difference between new and old is that new stores those NOT YET sent over the network
        // to this connector's parent in the solution tree.
        // invariant: old_local and new_local have an empty intersection
        Self {
            subtree_solutions,
            subtree_id_to_index,
            old_local: Default::default(),
            new_local: Default::default(),
        }
    }
    // drain old_local to new_local, visiting all new additions to old_local
    pub(crate) fn iter_new_local_make_old(&mut self) -> impl Iterator<Item = Predicate> + '_ {
        let Self { old_local, new_local, .. } = self;
        new_local.drain().map(move |local| {
            // rely on invariant: empty intersection between old and new local sets
            assert!(old_local.insert(local.clone()));
            local
        })
    }
    // insert a solution for the given subtree ID,
    // AND update new_local to include any solutions that become
    // possible as a result of this new addition
    pub(crate) fn submit_and_digest_subtree_solution(
        &mut self,
        cu: &mut impl CuUndecided,
        subtree_id: SubtreeId,
        predicate: Predicate,
    ) {
        log!(cu.logger(), "++ new component solution {:?} {:?}", subtree_id, &predicate);
        let Self { subtree_solutions, new_local, old_local, subtree_id_to_index } = self;
        let index = subtree_id_to_index[&subtree_id];
        let was_new = subtree_solutions[index].insert(predicate.clone());
        if was_new {
            // This is a newly-added solution! update new_local
            // consider ALL consistent combinations of one element from each solution set
            // to our right or left in the solution-set vector
            // but with THIS PARTICULAR predicate from our own index.
            let left = 0..index;
            let right = (index + 1)..subtree_solutions.len();
            // iterator over SETS of solutions, one for every component except `subtree_id` (me)
            let set_visitor = left.chain(right).map(|index| &subtree_solutions[index]);
            // Recursively enumerate all solutions matching the description above,
            Self::elaborate_into_new_local_rec(cu, predicate, set_visitor, old_local, new_local);
        }
    }

    // Recursively build local solutions for this connector,
    // see `submit_and_digest_subtree_solution`
    fn elaborate_into_new_local_rec<'a, 'b>(
        cu: &mut impl CuUndecided,
        partial: Predicate,
        mut set_visitor: impl Iterator<Item = &'b HashSet<Predicate>> + Clone,
        old_local: &'b HashSet<Predicate>,
        new_local: &'a mut HashSet<Predicate>,
    ) {
        if let Some(set) = set_visitor.next() {
            // incomplete solution. keep recursively creating combined solutions
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
            // recursive stop condition. This is a solution for this connector...
            if !old_local.contains(&partial) {
                // ... and it hasn't been found before
                log!(cu.logger(), "storing NEW LOCAL SOLUTION {:?}", &partial);
                new_local.insert(partial);
            }
        }
    }
}
impl NonsyncProtoContext<'_> {
    // Facilitates callback from the component to the connector runtime,
    // creating a new component and changing the given port's ownership to that
    // of the new component.
    pub(crate) fn new_component(&mut self, moved_ports: HashSet<PortId>, state: ComponentState) {
        // Sanity check! The moved ports are owned by this component to begin with
        for port in moved_ports.iter() {
            assert_eq!(self.proto_component_id, self.ips.port_info.map.get(port).unwrap().owner);
        }
        // Create the new component, and schedule it to be run
        let new_cid = self.ips.id_manager.new_component_id();
        log!(
            self.logger,
            "Component {:?} added new component {:?} with state {:?}, moving ports {:?}",
            self.proto_component_id,
            new_cid,
            &state,
            &moved_ports
        );
        self.unrun_components.push((new_cid, state));
        // Update the ownership of the moved ports
        for port in moved_ports.iter() {
            self.ips.port_info.map.get_mut(port).unwrap().owner = new_cid;
        }
        if let Some(set) = self.ips.port_info.owned.get_mut(&self.proto_component_id) {
            set.retain(|x| !moved_ports.contains(x));
        }
        self.ips.port_info.owned.insert(new_cid, moved_ports.clone());
    }

    // Facilitates callback from the component to the connector runtime,
    // creating a new port-pair connected by an memory channel
    pub(crate) fn new_port_pair(&mut self) -> [PortId; 2] {
        // adds two new associated ports, related to each other, and exposed to the proto component
        let mut new_cid_fn = || self.ips.id_manager.new_port_id();
        let [o, i] = [new_cid_fn(), new_cid_fn()];
        self.ips.port_info.map.insert(
            o,
            PortInfo {
                route: Route::LocalComponent,
                peer: Some(i),
                polarity: Putter,
                owner: self.proto_component_id,
            },
        );
        self.ips.port_info.map.insert(
            i,
            PortInfo {
                route: Route::LocalComponent,
                peer: Some(o),
                polarity: Getter,
                owner: self.proto_component_id,
            },
        );
        self.ips
            .port_info
            .owned
            .entry(self.proto_component_id)
            .or_default()
            .extend([o, i].iter().copied());
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
    // The component calls the runtime back, inspecting whether it's associated
    // preidcate has already determined a (speculative) value for the given port's firing variable.
    pub(crate) fn is_firing(&mut self, port: PortId) -> Option<bool> {
        let var = self.rctx.ips.port_info.spec_var_for(port);
        self.predicate.query(var).map(SpecVal::is_firing)
    }

    pub(crate) fn did_put_or_get(&mut self, port: PortId) -> bool {
        self.branch_inner.did_put_or_get.contains(&port)
    }

    // The component calls the runtime back, trying to inspect a port's message
    pub(crate) fn read_msg(&mut self, port: PortId) -> Option<&Payload> {
        let maybe_msg = self.branch_inner.inbox.get(&port);
        if maybe_msg.is_some() {
            // Make a note that this component has received
            // this port's message 1+ times this round
            self.branch_inner.did_put_or_get.insert(port);
        }
        maybe_msg
    }
}
