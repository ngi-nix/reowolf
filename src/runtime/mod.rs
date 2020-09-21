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

#[derive(Debug)]
pub struct Connector {
    unphased: ConnectorUnphased,
    phased: ConnectorPhased,
}
pub trait Logger: Debug + Send + Sync {
    fn line_writer(&mut self) -> Option<&mut dyn std::io::Write>;
}
#[derive(Debug)]
pub struct VecLogger(ConnectorId, Vec<u8>);
#[derive(Debug)]
pub struct DummyLogger;
#[derive(Debug)]
pub struct FileLogger(ConnectorId, std::fs::File);
#[derive(Debug, Clone)]
struct CurrentState {
    port_info: HashMap<PortId, PortInfo>,
    id_manager: IdManager,
}
pub(crate) struct NonsyncProtoContext<'a> {
    current_state: &'a mut CurrentState,
    logger: &'a mut dyn Logger,
    // cu_inner: &'a mut ConnectorUnphasedInner, // persists between rounds
    unrun_components: &'a mut Vec<(ComponentId, ComponentState)>, // lives for Nonsync phase
    proto_component_id: ComponentId,                              // KEY in id->component map
}
pub(crate) struct SyncProtoContext<'a> {
    rctx: &'a RoundCtx,
    branch_inner: &'a mut ProtoComponentBranchInner, // sub-structure of component branch
    predicate: &'a Predicate,                        // KEY in pred->branch map
}
#[derive(Default, Debug, Clone)]
struct ProtoComponentBranchInner {
    untaken_choice: Option<u16>,
    did_put_or_get: HashSet<PortId>,
    inbox: HashMap<PortId, Payload>,
}
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, Hash, PartialOrd, serde::Serialize, serde::Deserialize,
)]
struct SpecVar(PortId);
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, Hash, PartialOrd, serde::Serialize, serde::Deserialize,
)]
struct SpecVal(u16);
#[derive(Debug)]
struct RoundOk {
    batch_index: usize,
    gotten: HashMap<PortId, Payload>,
}
#[derive(Default)]
struct VecSet<T: std::cmp::Ord> {
    // invariant: ordered, deduplicated
    vec: Vec<T>,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
enum Route {
    LocalComponent,
    NetEndpoint { index: usize },
    UdpEndpoint { index: usize },
}
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
enum SubtreeId {
    LocalComponent(ComponentId),
    NetEndpoint { index: usize },
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct MyPortInfo {
    polarity: Polarity,
    port: PortId,
    owner: ComponentId,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum Decision {
    Failure,
    Success(Predicate),
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
enum Msg {
    SetupMsg(SetupMsg),
    CommMsg(CommMsg),
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
enum SetupMsg {
    MyPortInfo(MyPortInfo),
    LeaderWave { wave_leader: ConnectorId },
    LeaderAnnounce { tree_leader: ConnectorId },
    YouAreMyParent,
    SessionGather { unoptimized_map: HashMap<ConnectorId, SessionInfo> },
    SessionScatter { optimized_map: HashMap<ConnectorId, SessionInfo> },
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct SessionInfo {
    serde_proto_description: SerdeProtocolDescription,
    port_info: HashMap<PortId, PortInfo>,
    endpoint_incoming_to_getter: Vec<PortId>,
    proto_components: HashMap<ComponentId, ComponentState>,
}
#[derive(Debug, Clone)]
struct SerdeProtocolDescription(Arc<ProtocolDescription>);
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
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
enum CommCtrlMsg {
    Suggest { suggestion: Decision }, // SINKWARD
    Announce { decision: Decision },  // SINKAWAYS
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct SendPayloadMsg {
    predicate: Predicate,
    payload: Payload,
}
#[derive(Debug, PartialEq)]
enum AssignmentUnionResult {
    FormerNotLatter,
    LatterNotFormer,
    Equivalent,
    New(Predicate),
    Nonexistant,
}
struct NetEndpoint {
    inbox: Vec<u8>,
    stream: TcpStream,
}
#[derive(Debug, Clone)]
struct NetEndpointSetup {
    getter_for_incoming: PortId,
    sock_addr: SocketAddr,
    endpoint_polarity: EndpointPolarity,
}

#[derive(Debug, Clone)]
struct UdpEndpointSetup {
    getter_for_incoming: PortId,
    local_addr: SocketAddr,
    peer_addr: SocketAddr,
}
#[derive(Debug)]
struct NetEndpointExt {
    net_endpoint: NetEndpoint,
    getter_for_incoming: PortId,
}
#[derive(Debug)]
struct UdpEndpointExt {
    sock: UdpSocket, // already bound and connected
    received_this_round: bool,
    outgoing_payloads: HashMap<Predicate, Payload>,
    getter_for_incoming: PortId,
}
#[derive(Debug)]
struct Neighborhood {
    parent: Option<usize>,
    children: VecSet<usize>,
}
#[derive(Debug, Clone)]
struct IdManager {
    connector_id: ConnectorId,
    port_suffix_stream: U32Stream,
    component_suffix_stream: U32Stream,
}
struct UdpInBuffer {
    byte_vec: Vec<u8>,
}
#[derive(Debug)]
struct SpecVarStream {
    connector_id: ConnectorId,
    port_suffix_stream: U32Stream,
}
#[derive(Debug)]
struct EndpointManager {
    // invariants:
    // 1. net and udp endpoints are registered with poll. Poll token computed with TargetToken::into
    // 2. Events is empty
    poll: Poll,
    events: Events,
    delayed_messages: Vec<(usize, Msg)>,
    undelayed_messages: Vec<(usize, Msg)>,
    net_endpoint_store: EndpointStore<NetEndpointExt>,
    udp_endpoint_store: EndpointStore<UdpEndpointExt>,
    udp_in_buffer: UdpInBuffer,
}
#[derive(Debug)]
struct EndpointStore<T> {
    endpoint_exts: Vec<T>,
    polled_undrained: VecSet<usize>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct PortInfo {
    owner: ComponentId,
    peer: Option<PortId>,
    polarity: Polarity,
    route: Route,
}

#[derive(Debug)]
struct ConnectorCommunication {
    round_index: usize,
    endpoint_manager: EndpointManager,
    neighborhood: Neighborhood,
    native_batches: Vec<NativeBatch>,
    round_result: Result<Option<RoundOk>, SyncError>,
}
#[derive(Debug)]
struct ConnectorUnphased {
    proto_description: Arc<ProtocolDescription>,
    proto_components: HashMap<ComponentId, ComponentState>,
    inner: ConnectorUnphasedInner,
}
#[derive(Debug)]
struct ConnectorUnphasedInner {
    logger: Box<dyn Logger>,
    current_state: CurrentState,
    native_component_id: ComponentId,
}
#[derive(Debug)]
struct ConnectorSetup {
    net_endpoint_setups: Vec<NetEndpointSetup>,
    udp_endpoint_setups: Vec<UdpEndpointSetup>,
}
#[derive(Debug)]
enum ConnectorPhased {
    Setup(Box<ConnectorSetup>),
    Communication(Box<ConnectorCommunication>),
}
#[derive(Default, Clone, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
struct Predicate {
    assigned: BTreeMap<SpecVar, SpecVal>,
}
#[derive(Debug)]
struct SolutionStorage {
    old_local: HashSet<Predicate>,
    new_local: HashSet<Predicate>,
    // this pair acts as SubtreeId -> HashSet<Predicate> which is friendlier to iteration
    subtree_solutions: Vec<HashSet<Predicate>>,
    subtree_id_to_index: HashMap<SubtreeId, usize>,
}
struct RoundCtx {
    solution_storage: SolutionStorage,
    spec_var_stream: SpecVarStream,
    payload_inbox: Vec<(PortId, SendPayloadMsg)>,
    deadline: Option<Instant>,
    current_state: CurrentState,
}
trait CuUndecided {
    fn logger(&mut self) -> &mut dyn Logger;
    fn proto_description(&self) -> &ProtocolDescription;
    fn native_component_id(&self) -> ComponentId;
    fn logger_and_protocol_description(&mut self) -> (&mut dyn Logger, &ProtocolDescription);
}
#[derive(Debug, Default)]
struct NativeBatch {
    // invariant: putters' and getters' polarities respected
    to_put: HashMap<PortId, Payload>,
    to_get: HashSet<PortId>,
}
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
enum TokenTarget {
    NetEndpoint { index: usize },
    UdpEndpoint { index: usize },
    Waker,
}
enum CommRecvOk {
    TimeoutWithoutNew,
    NewPayloadMsgs,
    NewControlMsg { net_index: usize, msg: CommCtrlMsg },
}
////////////////
fn would_block(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::WouldBlock
}
impl TokenTarget {
    const HALFWAY_INDEX: usize = usize::MAX / 2;
    const MAX_INDEX: usize = usize::MAX;
    const WAKER_TOKEN: usize = Self::MAX_INDEX;
}
impl From<Token> for TokenTarget {
    fn from(Token(index): Token) -> Self {
        if index == Self::WAKER_TOKEN {
            TokenTarget::Waker
        } else if let Some(shifted) = index.checked_sub(Self::HALFWAY_INDEX) {
            TokenTarget::UdpEndpoint { index: shifted }
        } else {
            TokenTarget::NetEndpoint { index }
        }
    }
}
impl Into<Token> for TokenTarget {
    fn into(self) -> Token {
        match self {
            TokenTarget::Waker => Token(Self::WAKER_TOKEN),
            TokenTarget::UdpEndpoint { index } => Token(index + Self::HALFWAY_INDEX),
            TokenTarget::NetEndpoint { index } => Token(index),
        }
    }
}
impl<T: std::cmp::Ord> VecSet<T> {
    fn new(mut vec: Vec<T>) -> Self {
        vec.sort();
        vec.dedup();
        Self { vec }
    }
    fn contains(&self, element: &T) -> bool {
        self.vec.binary_search(element).is_ok()
    }
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
impl CurrentState {
    fn spec_var_for(&self, port: PortId) -> SpecVar {
        let info = self.port_info.get(&port).unwrap();
        SpecVar(match info.polarity {
            Getter => port,
            Putter => info.peer.unwrap(),
        })
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
        log!(&mut *self.unphased.inner.logger, "Connector dropping. Goodbye!");
    }
}
impl Connector {
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
    pub(crate) fn random_id() -> ConnectorId {
        type Bytes8 = [u8; std::mem::size_of::<ConnectorId>()];
        unsafe {
            let mut bytes = std::mem::MaybeUninit::<Bytes8>::uninit();
            // getrandom is the canonical crate for a small, secure rng
            getrandom::getrandom(&mut *bytes.as_mut_ptr()).unwrap();
            // safe! representations of all valid Byte8 values are valid ConnectorId values
            std::mem::transmute::<_, _>(bytes.assume_init())
        }
    }
    pub fn swap_logger(&mut self, mut new_logger: Box<dyn Logger>) -> Box<dyn Logger> {
        std::mem::swap(&mut self.unphased.inner.logger, &mut new_logger);
        new_logger
    }
    pub fn get_logger(&mut self) -> &mut dyn Logger {
        &mut *self.unphased.inner.logger
    }
    pub fn new_port_pair(&mut self) -> [PortId; 2] {
        let cu = &mut self.unphased;
        // adds two new associated ports, related to each other, and exposed to the native
        let mut new_cid = || cu.inner.current_state.id_manager.new_port_id();
        let [o, i] = [new_cid(), new_cid()];
        cu.inner.current_state.port_info.insert(
            o,
            PortInfo {
                route: Route::LocalComponent,
                peer: Some(i),
                owner: cu.inner.native_component_id,
                polarity: Putter,
            },
        );
        cu.inner.current_state.port_info.insert(
            i,
            PortInfo {
                route: Route::LocalComponent,
                peer: Some(o),
                owner: cu.inner.native_component_id,
                polarity: Getter,
            },
        );
        log!(cu.inner.logger, "Added port pair (out->in) {:?} -> {:?}", o, i);
        [o, i]
    }
    pub fn add_component(
        &mut self,
        identifier: &[u8],
        ports: &[PortId],
    ) -> Result<(), AddComponentError> {
        // called by the USER. moves ports owned by the NATIVE
        use AddComponentError as Ace;
        // 1. check if this is OK
        let cu = &mut self.unphased;
        let expected_polarities = cu.proto_description.component_polarities(identifier)?;
        if expected_polarities.len() != ports.len() {
            return Err(Ace::WrongNumberOfParamaters { expected: expected_polarities.len() });
        }
        for (&expected_polarity, &port) in expected_polarities.iter().zip(ports.iter()) {
            let info = cu.inner.current_state.port_info.get(&port).ok_or(Ace::UnknownPort(port))?;
            if info.owner != cu.inner.native_component_id {
                return Err(Ace::UnknownPort(port));
            }
            if info.polarity != expected_polarity {
                return Err(Ace::WrongPortPolarity { port, expected_polarity });
            }
        }
        // 2. add new component
        let new_cid = cu.inner.current_state.id_manager.new_component_id();
        cu.proto_components
            .insert(new_cid, cu.proto_description.new_main_component(identifier, ports));
        // 3. update port ownership
        for port in ports.iter() {
            match cu.inner.current_state.port_info.get_mut(port) {
                Some(port_info) => port_info.owner = new_cid,
                None => unreachable!(),
            }
        }
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

    pub fn assigns_subset(&self, maybe_superset: &Self) -> bool {
        for (var, val) in self.assigned.iter() {
            match maybe_superset.assigned.get(var) {
                Some(val2) if val2 == val => {}
                _ => return false, // var unmapped, or mapped differently
            }
        }
        true
    }

    // returns true IFF self.unify would return Equivalent OR FormerNotLatter
    // pub fn consistent_with(&self, other: &Self) -> bool {
    //     let [larger, smaller] =
    //         if self.assigned.len() > other.assigned.len() { [self, other] } else { [other, self] };

    //     for (var, val) in smaller.assigned.iter() {
    //         match larger.assigned.get(var) {
    //             Some(val2) if val2 != val => return false,
    //             _ => {}
    //         }
    //     }
    //     true
    // }

    /// Given self and other, two predicates, return the predicate whose
    /// assignments are the union of those of self and other.
    fn assignment_union(&self, other: &Self) -> AssignmentUnionResult {
        use AssignmentUnionResult as Aur;
        // iterators over assignments of both predicates. Rely on SORTED ordering of BTreeMap's keys.
        let [mut s_it, mut o_it] = [self.assigned.iter(), other.assigned.iter()];
        let [mut s, mut o] = [s_it.next(), o_it.next()];
        // lists of assignments in self but not other and vice versa.
        let [mut s_not_o, mut o_not_s] = [vec![], vec![]];
        loop {
            match [s, o] {
                [None, None] => break,
                [None, Some(x)] => {
                    o_not_s.push(x);
                    o_not_s.extend(o_it);
                    break;
                }
                [Some(x), None] => {
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
    pub fn union_with(&self, other: &Self) -> Option<Self> {
        let mut res = self.clone();
        for (&channel_id, &assignment_1) in other.assigned.iter() {
            match res.assigned.insert(channel_id, assignment_1) {
                Some(assignment_2) if assignment_1 != assignment_2 => return None,
                _ => {}
            }
        }
        Some(res)
    }
    pub fn query(&self, var: SpecVar) -> Option<SpecVal> {
        self.assigned.get(&var).copied()
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
impl Default for UdpInBuffer {
    fn default() -> Self {
        let mut byte_vec = Vec::with_capacity(Self::CAPACITY);
        unsafe {
            // safe! this vector is guaranteed to have sufficient capacity
            byte_vec.set_len(Self::CAPACITY);
        }
        Self { byte_vec }
    }
}
impl UdpInBuffer {
    const CAPACITY: usize = u16::MAX as usize;
    fn as_mut_slice(&mut self) -> &mut [u8] {
        self.byte_vec.as_mut_slice()
    }
}

impl Debug for UdpInBuffer {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "UdpInBuffer")
    }
}

impl RoundCtx {
    fn getter_pop(&mut self) -> Option<(PortId, SendPayloadMsg)> {
        self.payload_inbox.pop()
    }
    fn getter_push(&mut self, getter: PortId, msg: SendPayloadMsg) {
        self.payload_inbox.push((getter, msg));
    }
    fn putter_push(&mut self, cu: &mut impl CuUndecided, putter: PortId, msg: SendPayloadMsg) {
        if let Some(getter) = self.current_state.port_info.get(&putter).unwrap().peer {
            log!(cu.logger(), "Putter add (putter:{:?} => getter:{:?})", putter, getter);
            self.getter_push(getter, msg);
        } else {
            log!(cu.logger(), "Putter {:?} has no known peer!", putter);
            panic!("Putter {:?} has no known peer!");
        }
    }
}
