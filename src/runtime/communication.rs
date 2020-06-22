use super::*;
use crate::common::*;
use core::marker::PhantomData;

////////////////
struct BranchingNative {
    branches: HashMap<Predicate, NativeBranch>,
}
struct NativeBranch {
    index: usize,
    gotten: HashMap<PortId, Payload>,
    to_get: HashSet<PortId>,
}
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
        self.predicate.query(port)
    }
    pub fn read_msg(&mut self, port: PortId) -> Option<&Payload> {
        self.inbox.get(&port)
    }
}

impl Connector {
    pub fn sync(&mut self) -> Result<usize, SyncError> {
        use SyncError::*;
        match &mut self.phased {
            ConnectorPhased::Setup { .. } => Err(NotConnected),
            ConnectorPhased::Communication { native_batches, endpoint_manager, .. } => {
                // 1. run all proto components to Nonsync blockers
                let mut branching_proto_components =
                    HashMap::<ProtoComponentId, BranchingProtoComponent>::default();
                let mut unrun_components: Vec<(ProtoComponentId, ProtoComponent)> =
                    self.proto_components.iter().map(|(&k, v)| (k, v.clone())).collect();
                while let Some((proto_component_id, mut component)) = unrun_components.pop() {
                    // TODO coalesce fields
                    let mut ctx = NonsyncProtoContext {
                        logger: &mut *self.logger,
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
                    use NonsyncBlocker as B;
                    match component.state.nonsync_run(&mut ctx, &self.proto_description) {
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

                // (Putter, )
                let mut payload_outbox: Vec<(PortId, SendPayloadMsg)> = vec![];

                // 2. kick off the native
                let mut branching_native = BranchingNative { branches: Default::default() };
                for (index, NativeBatch { to_get, to_put }) in native_batches.drain(..).enumerate()
                {
                    let mut predicate = Predicate::default();
                    // assign trues
                    for &port in to_get.iter().chain(to_put.keys()) {
                        predicate.assigned.insert(port, true);
                    }
                    // assign falses
                    for &port in self.native_ports.iter() {
                        predicate.assigned.entry(port).or_insert(false);
                    }
                    // put all messages
                    for (port, payload) in to_put {
                        payload_outbox.push((
                            port,
                            SendPayloadMsg { payload_predicate: predicate.clone(), payload },
                        ));
                    }
                    let branch = NativeBranch { index, gotten: Default::default(), to_get };
                    if let Some(existing) = branching_native.branches.insert(predicate, branch) {
                        return Err(IndistinguishableBatches([index, existing.index]));
                    }
                }

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

                // run all proto components to their sync blocker
                for (proto_component_id, proto_component) in branching_proto_components.iter_mut() {
                    // run this component to sync blocker in-place
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
                                logger: &mut *self.logger,
                                predicate: &predicate,
                                proto_component_id: *proto_component_id,
                                inbox: &branch.inbox,
                            };
                            use SyncBlocker as B;
                            match branch.state.sync_run(&mut ctx, &self.proto_description) {
                                B::Inconsistent => {
                                    log!(self.logger, "Proto component {:?} branch with pred {:?} became inconsistent", proto_component_id, &predicate);
                                    // branch is inconsistent. throw it away
                                    drop((predicate, branch));
                                }
                                B::SyncBlockEnd => {
                                    // make concrete all variables
                                    for &port in proto_component.ports.iter() {
                                        predicate.assigned.entry(port).or_insert(false);
                                    }
                                    // submit solution for this component
                                    log!(self.logger, "Proto component {:?} branch with pred {:?} reached SyncBlockEnd", proto_component_id, &predicate);
                                    solution_storage.submit_and_digest_subtree_solution(
                                        &mut *self.logger,
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
                                    assert!(predicate.query(port).is_none());
                                    assert!(!branch.inbox.contains_key(&port));
                                    blocked.insert(predicate, branch);
                                }
                                B::CouldntCheckFiring(port) => {
                                    // sanity check
                                    assert!(predicate.query(port).is_none());
                                    let var = self.port_info.firing_var_for(port);
                                    // keep forks in "unblocked"
                                    unblocked_to.insert(
                                        predicate.clone().inserted(var, false),
                                        branch.clone(),
                                    );
                                    unblocked_to.insert(predicate.inserted(var, true), branch);
                                }
                                B::PutMsg(port, payload) => {
                                    // sanity check
                                    assert_eq!(Some(&Putter), self.port_info.polarities.get(&port));
                                    // overwrite assignment
                                    let var = self.port_info.firing_var_for(port);
                                    let was = predicate.assigned.insert(var, true);
                                    if was == Some(false) {
                                        log!(self.logger, "Proto component {:?} tried to PUT on port {:?} when pred said var {:?}==Some(false). inconsistent!", proto_component_id, port, var);
                                        // discard forever
                                        drop((predicate, branch));
                                    } else {
                                        // keep in "unblocked"
                                        payload_outbox.push((
                                            port,
                                            SendPayloadMsg {
                                                payload_predicate: predicate.clone(),
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
                // now all components are blocked!
                //

                let decision = 'undecided: loop {
                    // check if we already have a solution
                    for _solution in solution_storage.iter_new_local_make_old() {
                        // todo check if parent, inform children etc. etc.
                        break 'undecided Ok(0);
                    }

                    // send / recv messages
                };
                decision
            }
        }
    }
}
impl BranchingProtoComponent {
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
                log!(logger, "... storing NEW LOCAL SOLUTION {:?}", &partial);
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
