/// cbindgen:ignore
mod communication;
/// cbindgen:ignore
mod endpoints;
pub mod error;
/// cbindgen:ignore
mod logging;
/// cbindgen:ignore
mod setup;

#[cfg(test)]
mod tests;

use crate::common::*;
use error::*;
use mio::net::UdpSocket;

/// The interface between the user's application and a communication session,
/// in which the application plays the part of a (native) component. This structure provides the application
/// with functionality available to all components: the ability to add new channels (port pairs), and to
/// instantiate new components whose definitions are defined in the connector's configured protocol
/// description. Native components have the additional ability to add `dangling' ports backed by local/remote
/// IP addresses, to be coupled with a counterpart once the connector's setup is completed by `connect`.
/// This allows sets of applications to cooperate in constructing shared sessions that span the network.
#[derive(Debug)]
pub struct Connector {
    unphased: ConnectorUnphased,
    phased: ConnectorPhased,
}

/// Characterizes a type which can write lines of logging text.
/// The implementations provided in the `logging` module are likely to be sufficient,
/// but for added flexibility, users are able to implement their own loggers for use
/// by connectors.
pub trait Logger: Debug + Send + Sync {
    fn line_writer(&mut self) -> Option<&mut dyn std::io::Write>;
}

/// A logger that appends the logged strings to a growing byte buffer
#[derive(Debug)]
pub struct VecLogger(ConnectorId, Vec<u8>);

/// A trivial logger that always returns None, such that no logging information is ever written.
#[derive(Debug)]
pub struct DummyLogger;

/// A logger that writes the logged lines to a given file.
#[derive(Debug)]
pub struct FileLogger(ConnectorId, std::fs::File);

// Interface between protocol state and the connector runtime BEFORE all components
// ave begun their branching speculation. See ComponentState::nonsync_run.
pub(crate) struct NonsyncProtoContext<'a> {
    ips: &'a mut IdAndPortState,
    logger: &'a mut dyn Logger,
    unrun_components: &'a mut Vec<(ComponentId, ComponentState)>, // lives for Nonsync phase
    proto_component_id: ComponentId,                              // KEY in id->component map
}

// Interface between protocol state and the connector runtime AFTER all components
// have begun their branching speculation. See ComponentState::sync_run.
pub(crate) struct SyncProtoContext<'a> {
    rctx: &'a RoundCtx,
    branch_inner: &'a mut ProtoComponentBranchInner, // sub-structure of component branch
    predicate: &'a Predicate,                        // KEY in pred->branch map
}

// The data coupled with a particular protocol component branch, but crucially omitting
// the `ComponentState` such that this may be passed by reference to the state with separate
// access control.
#[derive(Default, Debug, Clone)]
struct ProtoComponentBranchInner {
    did_put_or_get: HashSet<PortId>,
    inbox: HashMap<PortId, Payload>,
}

// A speculative variable that lives for the duration of the synchronous round.
// Each is assigned a value in domain `SpecVal`.
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, Hash, PartialOrd, serde::Serialize, serde::Deserialize,
)]
struct SpecVar(PortId);

// The codomain of SpecVal. Has two associated constants for values FIRING and SILENT,
// but may also enumerate many more values to facilitate finer-grained nondeterministic branching.
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, Hash, PartialOrd, serde::Serialize, serde::Deserialize,
)]
struct SpecVal(u16);

// Data associated with a successful synchronous round, retained afterwards such that the
// native component can freely reflect on how it went, reading the messages received at their
// inputs, and reflecting on which of their connector's synchronous batches succeeded.
#[derive(Debug)]
struct RoundEndedNative {
    batch_index: usize,
    gotten: HashMap<PortId, Payload>,
}

// Implementation of a set in terms of a vector (optimized for reading, not writing)
#[derive(Default)]
struct VecSet<T: std::cmp::Ord> {
    // invariant: ordered, deduplicated
    vec: Vec<T>,
}

// Allows a connector to remember how to forward payloads towards the component that
// owns their destination port. `LocalComponent` corresponds with messages for components
// managed by the connector itself (hinting for it to look it up in a local structure),
// whereas the other variants direct the connector to forward the messages over the network.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
enum Route {
    LocalComponent,
    NetEndpoint { index: usize },
    UdpEndpoint { index: usize },
}

// The outcome of a synchronous round, representing the distributed consensus.
// In the success case, the attached predicate encodes a row in the session's trace table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum Decision {
    Failure, // some connector timed out!
    Success(Predicate),
}

// The type of control messages exchanged between connectors over the network
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
enum Msg {
    SetupMsg(SetupMsg),
    CommMsg(CommMsg),
}

// Control messages exchanged during the setup phase only
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
enum SetupMsg {
    MyPortInfo(MyPortInfo),
    LeaderWave { wave_leader: ConnectorId },
    LeaderAnnounce { tree_leader: ConnectorId },
    YouAreMyParent,
    SessionGather { unoptimized_map: HashMap<ConnectorId, SessionInfo> },
    SessionScatter { optimized_map: HashMap<ConnectorId, SessionInfo> },
}

// A data structure encoding the state of a connector, passed around
// during the session optimization procedure.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct SessionInfo {
    serde_proto_description: SerdeProtocolDescription,
    port_info: PortInfoMap,
    endpoint_incoming_to_getter: Vec<PortId>,
    proto_components: HashMap<ComponentId, ComponentState>,
}

// Newtype wrapper for an Arc<ProtocolDescription>,
// such that it can be (de)serialized for transmission over the network.
#[derive(Debug, Clone)]
struct SerdeProtocolDescription(Arc<ProtocolDescription>);

// Control message particular to the communication phase.
// as such, it's annotated with a round_index
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct CommMsg {
    round_index: usize,
    contents: CommMsgContents,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
enum CommMsgContents {
    SendPayload(SendPayloadMsg),
    CommCtrl(CommCtrlMsg),
}

// Connector <-> connector control messages for use in the communication phase
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
enum CommCtrlMsg {
    Suggest { suggestion: Decision }, // child->parent
    Announce { decision: Decision },  // parent->child
}

// Speculative payload message, communicating the value for the given
// port's message predecated on the given speculative variable assignments.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct SendPayloadMsg {
    predicate: Predicate,
    payload: Payload,
}

// Return result of `Predicate::assignment_union`, communicating the contents
// of the predicate which represents the (consistent) union of their mappings,
// if it exists (no variable mapped distinctly by the input predicates)
#[derive(Debug, PartialEq)]
enum AssignmentUnionResult {
    FormerNotLatter,
    LatterNotFormer,
    Equivalent,
    New(Predicate),
    Nonexistant,
}

// One of two endpoints for a control channel with a connector on either end.
// The underlying transport is TCP, so we use an inbox buffer to allow
// discrete payload receipt.
struct NetEndpoint {
    inbox: Vec<u8>,
    stream: TcpStream,
}

// Datastructure used during the setup phase representing a NetEndpoint TO BE SETUP
#[derive(Debug, Clone)]
struct NetEndpointSetup {
    getter_for_incoming: PortId,
    sock_addr: SocketAddr,
    endpoint_polarity: EndpointPolarity,
}

// Datastructure used during the setup phase representing a UdpEndpoint TO BE SETUP
#[derive(Debug, Clone)]
struct UdpEndpointSetup {
    getter_for_incoming: PortId,
    local_addr: SocketAddr,
    peer_addr: SocketAddr,
}

// NetEndpoint annotated with the ID of the port that receives payload
// messages received through the endpoint. This approach assumes that NetEndpoints
// DO NOT multiplex port->port channels, and so a mapping such as this is possible.
// As a result, the messages themselves don't need to carry the PortID with them.
#[derive(Debug)]
struct NetEndpointExt {
    net_endpoint: NetEndpoint,
    getter_for_incoming: PortId,
}

// Endpoint for a "raw" UDP endpoint. Corresponds to the "Udp Mediator Component"
// described in the literature.
// It acts as an endpoint by receiving messages via the poller etc. (managed by EndpointManager),
// It acts as a native component by managing a (speculative) set of payload messages (an outbox,
//  protecting the peer on the other side of the network).
#[derive(Debug)]
struct UdpEndpointExt {
    sock: UdpSocket, // already bound and connected
    received_this_round: bool,
    outgoing_payloads: HashMap<Predicate, Payload>,
    getter_for_incoming: PortId,
}

// Meta-data for the connector: its role in the consensus tree.
#[derive(Debug)]
struct Neighborhood {
    parent: Option<usize>,
    children: VecSet<usize>,
}

// Manages the connector's ID, and manages allocations for connector/port IDs.
#[derive(Debug, Clone)]
struct IdManager {
    connector_id: ConnectorId,
    port_suffix_stream: U32Stream,
    component_suffix_stream: U32Stream,
}

// Newtype wrapper around a byte buffer, used for UDP mediators to receive incoming datagrams.
struct IoByteBuffer {
    byte_vec: Vec<u8>,
}

// A generator of speculative variables. Created on-demand during the synchronous round
// by the IdManager.
#[derive(Debug)]
struct SpecVarStream {
    connector_id: ConnectorId,
    port_suffix_stream: U32Stream,
}

// Manages the messy state of the various endpoints, pollers, buffers, etc.
#[derive(Debug)]
struct EndpointManager {
    // invariants:
    // 1. net and udp endpoints are registered with poll with tokens computed with TargetToken::into
    // 2. Events is empty
    poll: Poll,
    events: Events,
    delayed_messages: Vec<(usize, Msg)>,
    undelayed_messages: Vec<(usize, Msg)>, // ready to yield
    net_endpoint_store: EndpointStore<NetEndpointExt>,
    udp_endpoint_store: EndpointStore<UdpEndpointExt>,
    io_byte_buffer: IoByteBuffer,
}

// A storage of endpoints, which keeps track of which components have raised
// an event during poll(), signifying that they need to be checked for new incoming data
#[derive(Debug)]
struct EndpointStore<T> {
    endpoint_exts: Vec<T>,
    polled_undrained: VecSet<usize>,
}

// The information associated with a port identifier, designed for local storage.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct PortInfo {
    owner: ComponentId,
    peer: Option<PortId>,
    polarity: Polarity,
    route: Route,
}

// Similar to `PortInfo`, but designed for communication during the setup procedure.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct MyPortInfo {
    polarity: Polarity,
    port: PortId,
    owner: ComponentId,
}

// Newtype around port info map, allowing the implementation of some
// useful methods
#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PortInfoMap {
    // invariant: self.invariant_preserved()
    // `owned` is redundant information, allowing for fast lookup
    // of a component's owned ports (which occurs during the sync round a lot)
    map: HashMap<PortId, PortInfo>,
    owned: HashMap<ComponentId, HashSet<PortId>>,
}

// A convenient substructure for containing port info and the ID manager.
// Houses the bulk of the connector's persistent state between rounds.
// It turns out several situations require access to both things.
#[derive(Debug, Clone)]
struct IdAndPortState {
    port_info: PortInfoMap,
    id_manager: IdManager,
}

// A component's setup-phase-specific data
#[derive(Debug)]
struct ConnectorCommunication {
    round_index: usize,
    endpoint_manager: EndpointManager,
    neighborhood: Neighborhood,
    native_batches: Vec<NativeBatch>,
    round_result: Result<Option<RoundEndedNative>, SyncError>,
}

// A component's data common to both setup and communication phases
#[derive(Debug)]
struct ConnectorUnphased {
    proto_description: Arc<ProtocolDescription>,
    proto_components: HashMap<ComponentId, ComponentState>,
    logger: Box<dyn Logger>,
    ips: IdAndPortState,
    native_component_id: ComponentId,
}

// A connector's phase-specific data
#[derive(Debug)]
enum ConnectorPhased {
    Setup(Box<ConnectorSetup>),
    Communication(Box<ConnectorCommunication>),
}

// A connector's setup-phase-specific data
#[derive(Debug)]
struct ConnectorSetup {
    net_endpoint_setups: Vec<NetEndpointSetup>,
    udp_endpoint_setups: Vec<UdpEndpointSetup>,
}

// A newtype wrapper for a map from speculative variable to speculative value
// A missing mapping corresponds with "unspecified".
#[derive(Default, Clone, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
struct Predicate {
    assigned: BTreeMap<SpecVar, SpecVal>,
}

// Identifies a child of this connector in the _solution tree_.
// Each connector creates its own local solutions for the consensus procedure during `sync`,
// from the solutions of its children. Those children are either locally-managed components,
// (which are leaves in the solution tree), or other connectors reachable through the given
// network endpoint (which are internal nodes in the solution tree).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
enum SubtreeId {
    LocalComponent(ComponentId),
    NetEndpoint { index: usize },
}

// An accumulation of the connector's knowledge of all (a) the local solutions its children
// in the solution tree have found, and (b) its own solutions derivable from those of its children.
// This structure starts off each round with an empty set, and accumulates solutions as they are found
// by local components, or received over the network in control messages.
// IMPORTANT: solutions, once found, don't go away until the end of the round. That is to
// say that these sets GROW until the round is over, and all solutions are reset.
#[derive(Debug)]
struct SolutionStorage {
    // invariant: old_local U new_local solutions are those that can be created from
    // the UNION of one element from each set in `subtree_solution`.
    // invariant is maintained by potentially populating new_local whenever subtree_solutions is populated.
    old_local: HashSet<Predicate>, // already sent to this connector's parent OR decided
    new_local: HashSet<Predicate>, // not yet sent to this connector's parent OR decided
    // this pair acts as SubtreeId -> HashSet<Predicate> which is friendlier to iteration
    subtree_solutions: Vec<HashSet<Predicate>>,
    subtree_id_to_index: HashMap<SubtreeId, usize>,
}

// Stores the transient data of a synchronous round.
// Some of it is for bookkeeping, and the rest is a temporary mirror of fields of
// `ConnectorUnphased`, such that any changes are safely contained within RoundCtx,
// and can be undone if the round fails.
struct RoundCtx {
    solution_storage: SolutionStorage,
    spec_var_stream: SpecVarStream,
    payload_inbox: Vec<(PortId, SendPayloadMsg)>,
    deadline: Option<Instant>,
    ips: IdAndPortState,
}

// A trait intended to limit the access of the ConnectorUnphased structure
// such that we don't accidentally modify any important component/port data
// while the results of the round are undecided. Why? Any actions during Connector::sync
// are _speculative_ until the round is decided, and we need a safe way of rolling
// back any changes.
trait CuUndecided {
    fn logger(&mut self) -> &mut dyn Logger;
    fn proto_description(&self) -> &ProtocolDescription;
    fn native_component_id(&self) -> ComponentId;
    fn logger_and_protocol_description(&mut self) -> (&mut dyn Logger, &ProtocolDescription);
    fn logger_and_protocol_components(
        &mut self,
    ) -> (&mut dyn Logger, &mut HashMap<ComponentId, ComponentState>);
}

// Represents a set of synchronous port operations that the native component
// has described as an "option" for completing during the synchronous rounds.
// Operations contained here succeed together or not at all.
// A native with N=2+ batches are expressing an N-way nondeterministic choice
#[derive(Debug, Default)]
struct NativeBatch {
    // invariant: putters' and getters' polarities respected
    to_put: HashMap<PortId, Payload>,
    to_get: HashSet<PortId>,
}

// Parallels a mio::Token type, but more clearly communicates
// the way it identifies the evented structre it corresponds to.
// See runtime/setup for methods converting between TokenTarget and mio::Token
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
enum TokenTarget {
    NetEndpoint { index: usize },
    UdpEndpoint { index: usize },
}

// Returned by the endpoint manager as a result of comm_recv, telling the connector what happened,
// such that it can know when to continue polling, and when to block.
enum CommRecvOk {
    TimeoutWithoutNew,
    NewPayloadMsgs,
    NewControlMsg { net_index: usize, msg: CommCtrlMsg },
}
////////////////
fn err_would_block(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::WouldBlock
}
impl<T: std::cmp::Ord> VecSet<T> {
    fn new(mut vec: Vec<T>) -> Self {
        // establish the invariant
        vec.sort();
        vec.dedup();
        Self { vec }
    }
    fn contains(&self, element: &T) -> bool {
        self.vec.binary_search(element).is_ok()
    }
    // Insert the given element. Returns whether it was already present.
    fn insert(&mut self, element: T) -> bool {
        match self.vec.binary_search(&element) {
            Ok(_) => false,
            Err(index) => {
                self.vec.insert(index, element);
                true
            }
        }
    }
    fn iter(&self) -> std::slice::Iter<T> {
        self.vec.iter()
    }
    fn pop(&mut self) -> Option<T> {
        self.vec.pop()
    }
}
impl PortInfoMap {
    fn ports_owned_by(&self, owner: ComponentId) -> impl Iterator<Item = &PortId> {
        self.owned.get(&owner).into_iter().flat_map(HashSet::iter)
    }
    fn spec_var_for(&self, port: PortId) -> SpecVar {
        // Every port maps to a speculative variable
        // Two distinct ports map to the same variable
        // IFF they are two ends of the same logical channel.
        let info = self.map.get(&port).unwrap();
        SpecVar(match info.polarity {
            Getter => port,
            Putter => info.peer.unwrap(),
        })
    }
    fn invariant_preserved(&self) -> bool {
        // for every port P with some owner O,
        // P is in O's owned set
        for (port, info) in self.map.iter() {
            match self.owned.get(&info.owner) {
                Some(set) if set.contains(port) => {}
                _ => {
                    println!("{:#?}\n WITH port {:?}", self, port);
                    return false;
                }
            }
        }
        // for every port P owned by every owner O,
        // P's owner is O
        for (&owner, set) in self.owned.iter() {
            for port in set {
                match self.map.get(port) {
                    Some(info) if info.owner == owner => {}
                    _ => {
                        println!("{:#?}\n WITH owner {:?} port {:?}", self, owner, port);
                        return false;
                    }
                }
            }
        }
        true
    }
}
impl SpecVarStream {
    fn next(&mut self) -> SpecVar {
        let phantom_port: PortId =
            Id { connector_id: self.connector_id, u32_suffix: self.port_suffix_stream.next() }
                .into();
        SpecVar(phantom_port)
    }
}
impl IdManager {
    fn new(connector_id: ConnectorId) -> Self {
        Self {
            connector_id,
            port_suffix_stream: Default::default(),
            component_suffix_stream: Default::default(),
        }
    }
    fn new_spec_var_stream(&self) -> SpecVarStream {
        // Spec var stream starts where the current port_id stream ends, with gap of SKIP_N.
        // This gap is entirely unnecessary (i.e. 0 is fine)
        // It's purpose is only to make SpecVars easier to spot in logs.
        // E.g. spot the spec var: { v0_0, v1_2, v1_103 }
        const SKIP_N: u32 = 100;
        let port_suffix_stream = self.port_suffix_stream.clone().n_skipped(SKIP_N);
        SpecVarStream { connector_id: self.connector_id, port_suffix_stream }
    }
    fn new_port_id(&mut self) -> PortId {
        Id { connector_id: self.connector_id, u32_suffix: self.port_suffix_stream.next() }.into()
    }
    fn new_component_id(&mut self) -> ComponentId {
        Id { connector_id: self.connector_id, u32_suffix: self.component_suffix_stream.next() }
            .into()
    }
}
impl Drop for Connector {
    fn drop(&mut self) {
        log!(self.unphased.logger(), "Connector dropping. Goodbye!");
    }
}
// Given a slice of ports, return the first, if any, port is present repeatedly
fn duplicate_port(slice: &[PortId]) -> Option<PortId> {
    let mut vec = Vec::with_capacity(slice.len());
    for port in slice.iter() {
        match vec.binary_search(port) {
            Err(index) => vec.insert(index, *port),
            Ok(_) => return Some(*port),
        }
    }
    None
}
impl Connector {
    /// Generate a random connector identifier from the system's source of randomness.
    pub fn random_id() -> ConnectorId {
        type Bytes8 = [u8; std::mem::size_of::<ConnectorId>()];
        unsafe {
            let mut bytes = std::mem::MaybeUninit::<Bytes8>::uninit();
            // getrandom is the canonical crate for a small, secure rng
            getrandom::getrandom(&mut *bytes.as_mut_ptr()).unwrap();
            // safe! representations of all valid Byte8 values are valid ConnectorId values
            std::mem::transmute::<_, _>(bytes.assume_init())
        }
    }

    /// Returns true iff the connector is in connected state, i.e., it's setup phase is complete,
    /// and it is ready to participate in synchronous rounds of communication.
    pub fn is_connected(&self) -> bool {
        // If designed for Rust usage, connectors would be exposed as an enum type from the start.
        // consequently, this "phased" business would also include connector variants and this would
        // get a lot closer to the connector impl. itself.
        // Instead, the C-oriented implementation doesn't distinguish connector states as types,
        // and distinguish them as enum variants instead
        match self.phased {
            ConnectorPhased::Setup(..) => false,
            ConnectorPhased::Communication(..) => true,
        }
    }

    /// Enables the connector's current logger to be swapped out for another
    pub fn swap_logger(&mut self, mut new_logger: Box<dyn Logger>) -> Box<dyn Logger> {
        std::mem::swap(&mut self.unphased.logger, &mut new_logger);
        new_logger
    }

    /// Access the connector's current logger
    pub fn get_logger(&mut self) -> &mut dyn Logger {
        &mut *self.unphased.logger
    }

    /// Create a new synchronous channel, returning its ends as a pair of ports,
    /// with polarity output, input respectively. Available during either setup/communication phase.
    /// # Panics
    /// This function panics if the connector's (large) port id space is exhausted.
    pub fn new_port_pair(&mut self) -> [PortId; 2] {
        let cu = &mut self.unphased;
        // adds two new associated ports, related to each other, and exposed to the native
        let mut new_cid = || cu.ips.id_manager.new_port_id();
        // allocate two fresh port identifiers
        let [o, i] = [new_cid(), new_cid()];
        // store info for each:
        // - they are each others' peers
        // - they are owned by a local component with id `cid`
        // - polarity putter, getter respectively
        cu.ips.port_info.map.insert(
            o,
            PortInfo {
                route: Route::LocalComponent,
                peer: Some(i),
                owner: cu.native_component_id,
                polarity: Putter,
            },
        );
        cu.ips.port_info.map.insert(
            i,
            PortInfo {
                route: Route::LocalComponent,
                peer: Some(o),
                owner: cu.native_component_id,
                polarity: Getter,
            },
        );
        cu.ips
            .port_info
            .owned
            .entry(cu.native_component_id)
            .or_default()
            .extend([o, i].iter().copied());

        log!(cu.logger, "Added port pair (out->in) {:?} -> {:?}", o, i);
        [o, i]
    }

    /// Instantiates a new component for the connector runtime to manage, and passing
    /// the given set of ports from the interface of the native component, to that of the
    /// newly created component (passing their ownership).
    /// # Errors
    /// Error is returned if the moved ports are not owned by the native component,
    /// if the given component name is not defined in the connector's protocol,
    /// the given sequence of ports contains a duplicate port,
    /// or if the component is unfit for instantiation with the given port sequence.
    /// # Panics
    /// This function panics if the connector's (large) component id space is exhausted.
    pub fn add_component(
        &mut self,
        identifier: &[u8],
        ports: &[PortId],
    ) -> Result<(), AddComponentError> {
        // Check for error cases first before modifying `cu`
        use AddComponentError as Ace;
        let cu = &self.unphased;
        if let Some(port) = duplicate_port(ports) {
            return Err(Ace::DuplicatePort(port));
        }
        let expected_polarities = cu.proto_description.component_polarities(identifier)?;
        if expected_polarities.len() != ports.len() {
            return Err(Ace::WrongNumberOfParamaters { expected: expected_polarities.len() });
        }
        for (&expected_polarity, &port) in expected_polarities.iter().zip(ports.iter()) {
            let info = cu.ips.port_info.map.get(&port).ok_or(Ace::UnknownPort(port))?;
            if info.owner != cu.native_component_id {
                return Err(Ace::UnknownPort(port));
            }
            if info.polarity != expected_polarity {
                return Err(Ace::WrongPortPolarity { port, expected_polarity });
            }
        }
        // No errors! Time to modify `cu`
        // create a new component and identifier
        let Connector { phased, unphased: cu } = self;
        let new_cid = cu.ips.id_manager.new_component_id();
        cu.proto_components.insert(new_cid, cu.proto_description.new_component(identifier, ports));
        // update the ownership of moved ports
        for port in ports.iter() {
            match cu.ips.port_info.map.get_mut(port) {
                Some(port_info) => port_info.owner = new_cid,
                None => unreachable!(),
            }
        }
        if let Some(set) = cu.ips.port_info.owned.get_mut(&cu.native_component_id) {
            set.retain(|x| !ports.contains(x));
        }
        let moved_port_set: HashSet<PortId> = ports.iter().copied().collect();
        if let ConnectorPhased::Communication(comm) = phased {
            // Preserve invariant: batches only reason about native's ports.
            // Remove batch puts/gets for moved ports.
            for batch in comm.native_batches.iter_mut() {
                batch.to_put.retain(|port, _| !moved_port_set.contains(port));
                batch.to_get.retain(|port| !moved_port_set.contains(port));
            }
        }
        cu.ips.port_info.owned.insert(new_cid, moved_port_set);
        Ok(())
    }
}
impl Predicate {
    #[inline]
    pub fn singleton(k: SpecVar, v: SpecVal) -> Self {
        Self::default().inserted(k, v)
    }
    #[inline]
    pub fn inserted(mut self, k: SpecVar, v: SpecVal) -> Self {
        self.assigned.insert(k, v);
        self
    }

    // Return true whether `self` is a subset of `maybe_superset`
    pub fn assigns_subset(&self, maybe_superset: &Self) -> bool {
        for (var, val) in self.assigned.iter() {
            match maybe_superset.assigned.get(var) {
                Some(val2) if val2 == val => {}
                _ => return false, // var unmapped, or mapped differently
            }
        }
        // `maybe_superset` mirrored all my assignments!
        true
    }

    /// Given the two predicates {self, other}, return that whose
    /// assignments are the union of those of both.
    fn assignment_union(&self, other: &Self) -> AssignmentUnionResult {
        use AssignmentUnionResult as Aur;
        // iterators over assignments of both predicates. Rely on SORTED ordering of BTreeMap's keys.
        let [mut s_it, mut o_it] = [self.assigned.iter(), other.assigned.iter()];
        let [mut s, mut o] = [s_it.next(), o_it.next()];
        // populate lists of assignments in self but not other and vice versa.
        // do this by incrementally unfolding the iterators, keeping an eye
        // on the ordering between the head elements [s, o].
        // whenever s<o, other is certainly missing element 's', etc.
        let [mut s_not_o, mut o_not_s] = [vec![], vec![]];
        loop {
            match [s, o] {
                [None, None] => break, // both iterators are empty
                [None, Some(x)] => {
                    // self's iterator is empty.
                    // all remaning elements are in other but not self
                    o_not_s.push(x);
                    o_not_s.extend(o_it);
                    break;
                }
                [Some(x), None] => {
                    // other's iterator is empty.
                    // all remaning elements are in self but not other
                    s_not_o.push(x);
                    s_not_o.extend(s_it);
                    break;
                }
                [Some((sid, sb)), Some((oid, ob))] => {
                    if sid < oid {
                        // o is missing this element
                        s_not_o.push((sid, sb));
                        s = s_it.next();
                    } else if sid > oid {
                        // s is missing this element
                        o_not_s.push((oid, ob));
                        o = o_it.next();
                    } else if sb != ob {
                        assert_eq!(sid, oid);
                        // both predicates assign the variable but differ on the value
                        // No predicate exists which satisfies both!
                        return Aur::Nonexistant;
                    } else {
                        // both predicates assign the variable to the same value
                        s = s_it.next();
                        o = o_it.next();
                    }
                }
            }
        }
        // Observed zero inconsistencies. A unified predicate exists...
        match [s_not_o.is_empty(), o_not_s.is_empty()] {
            [true, true] => Aur::Equivalent,       // ... equivalent to both.
            [false, true] => Aur::FormerNotLatter, // ... equivalent to self.
            [true, false] => Aur::LatterNotFormer, // ... equivalent to other.
            [false, false] => {
                // ... which is the union of the predicates' assignments but
                //     is equivalent to neither self nor other.
                let mut new = self.clone();
                for (&id, &b) in o_not_s {
                    new.assigned.insert(id, b);
                }
                Aur::New(new)
            }
        }
    }

    // Compute the union of the assignments of the two given predicates, if it exists.
    // It doesn't exist if there is some value which the predicates assign to different values.
    pub(crate) fn union_with(&self, other: &Self) -> Option<Self> {
        let mut res = self.clone();
        for (&channel_id, &assignment_1) in other.assigned.iter() {
            match res.assigned.insert(channel_id, assignment_1) {
                Some(assignment_2) if assignment_1 != assignment_2 => return None,
                _ => {}
            }
        }
        Some(res)
    }
    pub(crate) fn query(&self, var: SpecVar) -> Option<SpecVal> {
        self.assigned.get(&var).copied()
    }
}

impl RoundCtx {
    // remove an arbitrary buffered message, along with the ID of the getter who receives it
    fn getter_pop(&mut self) -> Option<(PortId, SendPayloadMsg)> {
        self.payload_inbox.pop()
    }

    // buffer a message along with the ID of the getter who receives it
    fn getter_push(&mut self, getter: PortId, msg: SendPayloadMsg) {
        self.payload_inbox.push((getter, msg));
    }

    // buffer a message along with the ID of the putter who sent it
    fn putter_push(&mut self, cu: &mut impl CuUndecided, putter: PortId, msg: SendPayloadMsg) {
        if let Some(getter) = self.ips.port_info.map.get(&putter).unwrap().peer {
            log!(cu.logger(), "Putter add (putter:{:?} => getter:{:?})", putter, getter);
            self.getter_push(getter, msg);
        } else {
            log!(cu.logger(), "Putter {:?} has no known peer!", putter);
            panic!("Putter {:?} has no known peer!", putter);
        }
    }
}

impl<T: Debug + std::cmp::Ord> Debug for VecSet<T> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        f.debug_set().entries(self.vec.iter()).finish()
    }
}
impl Debug for Predicate {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        struct Assignment<'a>((&'a SpecVar, &'a SpecVal));
        impl Debug for Assignment<'_> {
            fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
                write!(f, "{:?}={:?}", (self.0).0, (self.0).1)
            }
        }
        f.debug_set().entries(self.assigned.iter().map(Assignment)).finish()
    }
}
impl serde::Serialize for SerdeProtocolDescription {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let inner: &ProtocolDescription = &self.0;
        inner.serialize(serializer)
    }
}
impl<'de> serde::Deserialize<'de> for SerdeProtocolDescription {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let inner: ProtocolDescription = ProtocolDescription::deserialize(deserializer)?;
        Ok(Self(Arc::new(inner)))
    }
}
impl IdParts for SpecVar {
    fn id_parts(self) -> (ConnectorId, U32Suffix) {
        self.0.id_parts()
    }
}
impl Debug for SpecVar {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        let (a, b) = self.id_parts();
        write!(f, "v{}_{}", a, b)
    }
}
impl SpecVal {
    const FIRING: Self = SpecVal(1);
    const SILENT: Self = SpecVal(0);
    fn is_firing(self) -> bool {
        self == Self::FIRING
        // all else treated as SILENT
    }
    fn iter_domain() -> impl Iterator<Item = Self> {
        (0..).map(SpecVal)
    }
}
impl Debug for SpecVal {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl Default for IoByteBuffer {
    fn default() -> Self {
        let mut byte_vec = Vec::with_capacity(Self::CAPACITY);
        unsafe {
            // safe! this vector is guaranteed to have sufficient capacity
            byte_vec.set_len(Self::CAPACITY);
        }
        Self { byte_vec }
    }
}
impl IoByteBuffer {
    const CAPACITY: usize = u16::MAX as usize + 1000;
    fn as_mut_slice(&mut self) -> &mut [u8] {
        self.byte_vec.as_mut_slice()
    }
}

impl Debug for IoByteBuffer {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "IoByteBuffer")
    }
}
