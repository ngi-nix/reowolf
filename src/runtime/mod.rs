/// cbindgen:ignore
mod communication;
/// cbindgen:ignore
mod endpoints;
pub mod error;
/// cbindgen:ignore
mod logging;
/// cbindgen:ignore
mod setup;

#[cfg(feature = "ffi")]
pub mod ffi;

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
pub trait Logger: Debug {
    fn line_writer(&mut self) -> &mut dyn std::io::Write;
}
#[derive(Debug)]
pub struct VecLogger(ConnectorId, Vec<u8>);
#[derive(Debug)]
pub struct DummyLogger;
#[derive(Debug)]
pub struct FileLogger(ConnectorId, std::fs::File);
pub(crate) struct NonsyncProtoContext<'a> {
    logger: &'a mut dyn Logger,
    proto_component_id: ProtoComponentId,
    port_info: &'a mut PortInfo,
    id_manager: &'a mut IdManager,
    proto_component_ports: &'a mut HashSet<PortId>,
    unrun_components: &'a mut Vec<(ProtoComponentId, ProtoComponent)>,
}
pub(crate) struct SyncProtoContext<'a> {
    logger: &'a mut dyn Logger,
    untaken_choice: &'a mut Option<u16>,
    predicate: &'a Predicate,
    port_info: &'a PortInfo,
    inbox: &'a HashMap<PortId, Payload>,
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
enum ComponentId {
    Native,
    Proto(ProtoComponentId),
}
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
enum Route {
    LocalComponent(ComponentId),
    Endpoint { index: usize },
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct MyPortInfo {
    polarity: Polarity,
    port: PortId,
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
    port_info: PortInfo,
    endpoint_incoming_to_getter: Vec<PortId>,
    proto_components: HashMap<ProtoComponentId, ProtoComponent>,
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
    Suggest { suggestion: Decision }, // SINKWARD
    Announce { decision: Decision },  // SINKAWAYS
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct SendPayloadMsg {
    predicate: Predicate,
    payload: Payload,
}
#[derive(Debug, PartialEq)]
enum AllMapperResult {
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ProtoComponent {
    state: ComponentState,
    ports: HashSet<PortId>,
}
#[derive(Debug, Clone)]
struct NetEndpointSetup {
    sock_addr: SocketAddr,
    endpoint_polarity: EndpointPolarity,
}

#[derive(Debug, Clone)]
struct UdpEndpointSetup {
    local_addr: SocketAddr,
    peer_addr: SocketAddr,
}
#[derive(Debug)]
struct NetEndpointExt {
    net_endpoint: NetEndpoint,
    getter_for_incoming: PortId,
}
#[derive(Debug)]
struct Neighborhood {
    parent: Option<usize>,
    children: VecSet<usize>,
}
#[derive(Debug)]
struct IdManager {
    connector_id: ConnectorId,
    port_suffix_stream: U32Stream,
    proto_component_suffix_stream: U32Stream,
}
#[derive(Debug)]
struct SpecVarStream {
    connector_id: ConnectorId,
    port_suffix_stream: U32Stream,
}
#[derive(Debug)]
struct EndpointManager {
    // invariants:
    // 1. endpoint N is registered READ | WRITE with poller
    // 2. Events is empty
    poll: Poll,
    events: Events,
    polled_undrained: VecSet<usize>,
    delayed_messages: Vec<(usize, Msg)>,
    undelayed_messages: Vec<(usize, Msg)>,
    net_endpoint_exts: Vec<NetEndpointExt>,
}
struct UdpEndpointExt {
    sock: UdpSocket,
    getter_for_incoming: PortId,
    outgoing_buffer: HashMap<Predicate, Payload>,
    incoming_buffer: Vec<Payload>,
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct PortInfo {
    polarities: HashMap<PortId, Polarity>,
    peers: HashMap<PortId, PortId>,
    routes: HashMap<PortId, Route>,
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
    proto_components: HashMap<ProtoComponentId, ProtoComponent>,
    logger: Box<dyn Logger>,
    id_manager: IdManager,
    native_ports: HashSet<PortId>,
    port_info: PortInfo,
}
#[derive(Debug)]
struct ConnectorSetup {
    net_endpoint_setups: Vec<(PortId, NetEndpointSetup)>,
    udp_endpoint_setups: Vec<(PortId, UdpEndpointSetup)>,
    surplus_sockets: u16,
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
#[derive(Debug, Default)]
struct NativeBatch {
    // invariant: putters' and getters' polarities respected
    to_put: HashMap<PortId, Payload>,
    to_get: HashSet<PortId>,
}
enum TokenTarget {
    NetEndpoint { index: usize },
    UdpEndpoint { index: usize },
    Waker,
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
        if index == Self::MAX_INDEX {
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
            TokenTarget::Waker => Token(Self::MAX_INDEX),
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
impl PortInfo {
    fn spec_var_for(&self, port: PortId) -> SpecVar {
        SpecVar(match self.polarities.get(&port).unwrap() {
            Getter => port,
            Putter => *self.peers.get(&port).unwrap(),
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
            proto_component_suffix_stream: Default::default(),
        }
    }
    fn new_spec_var_stream(&self) -> SpecVarStream {
        SpecVarStream {
            connector_id: self.connector_id,
            port_suffix_stream: self.port_suffix_stream.clone(),
        }
    }
    fn new_port_id(&mut self) -> PortId {
        Id { connector_id: self.connector_id, u32_suffix: self.port_suffix_stream.next() }.into()
    }
    fn new_proto_component_id(&mut self) -> ProtoComponentId {
        Id {
            connector_id: self.connector_id,
            u32_suffix: self.proto_component_suffix_stream.next(),
        }
        .into()
    }
}
impl Drop for Connector {
    fn drop(&mut self) {
        log!(&mut *self.unphased.logger, "Connector dropping. Goodbye!");
    }
}
impl Connector {
    fn random_id() -> ConnectorId {
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
        std::mem::swap(&mut self.unphased.logger, &mut new_logger);
        new_logger
    }
    pub fn get_logger(&mut self) -> &mut dyn Logger {
        &mut *self.unphased.logger
    }
    pub fn new_port_pair(&mut self) -> [PortId; 2] {
        let cu = &mut self.unphased;
        // adds two new associated ports, related to each other, and exposed to the native
        let [o, i] = [cu.id_manager.new_port_id(), cu.id_manager.new_port_id()];
        cu.native_ports.insert(o);
        cu.native_ports.insert(i);
        // {polarity, peer, route} known. {} unknown.
        cu.port_info.polarities.insert(o, Putter);
        cu.port_info.polarities.insert(i, Getter);
        cu.port_info.peers.insert(o, i);
        cu.port_info.peers.insert(i, o);
        let route = Route::LocalComponent(ComponentId::Native);
        cu.port_info.routes.insert(o, route);
        cu.port_info.routes.insert(i, route);
        log!(cu.logger, "Added port pair (out->in) {:?} -> {:?}", o, i);
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
        let polarities = cu.proto_description.component_polarities(identifier)?;
        if polarities.len() != ports.len() {
            return Err(Ace::WrongNumberOfParamaters { expected: polarities.len() });
        }
        for (&expected_polarity, port) in polarities.iter().zip(ports.iter()) {
            if !cu.native_ports.contains(port) {
                return Err(Ace::UnknownPort(*port));
            }
            if expected_polarity != *cu.port_info.polarities.get(port).unwrap() {
                return Err(Ace::WrongPortPolarity { port: *port, expected_polarity });
            }
        }
        // 3. remove ports from old component & update port->route
        let new_id = cu.id_manager.new_proto_component_id();
        for port in ports.iter() {
            cu.port_info.routes.insert(*port, Route::LocalComponent(ComponentId::Proto(new_id)));
        }
        cu.native_ports.retain(|port| !ports.contains(port));
        // 4. add new component
        cu.proto_components.insert(
            new_id,
            ProtoComponent {
                state: cu.proto_description.new_main_component(identifier, ports),
                ports: ports.iter().copied().collect(),
            },
        );
        Ok(())
    }
}
impl Predicate {
    #[inline]
    pub fn inserted(mut self, k: SpecVar, v: SpecVal) -> Self {
        self.assigned.insert(k, v);
        self
    }
    // returns true IFF self.unify would return Equivalent OR FormerNotLatter
    pub fn consistent_with(&self, other: &Self) -> bool {
        let [larger, smaller] =
            if self.assigned.len() > other.assigned.len() { [self, other] } else { [other, self] };

        for (var, val) in smaller.assigned.iter() {
            match larger.assigned.get(var) {
                Some(val2) if val2 != val => return false,
                _ => {}
            }
        }
        true
    }

    /// Given self and other, two predicates, return the most general Predicate possible, N
    /// such that n.satisfies(self) && n.satisfies(other).
    /// If none exists Nonexistant is returned.
    /// If the resulting predicate is equivlanet to self, other, or both,
    /// FormerNotLatter, LatterNotFormer and Equivalent are returned respectively.
    /// otherwise New(N) is returned.
    fn all_mapper(&self, other: &Self) -> AllMapperResult {
        use AllMapperResult as Amr;
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
                        return Amr::Nonexistant;
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
            [true, true] => Amr::Equivalent,       // ... equivalent to both.
            [false, true] => Amr::FormerNotLatter, // ... equivalent to self.
            [true, false] => Amr::LatterNotFormer, // ... equivalent to other.
            [false, false] => {
                // ... which is the union of the predicates' assignments but
                //     is equivalent to neither self nor other.
                let mut new = self.clone();
                for (&id, &b) in o_not_s {
                    new.assigned.insert(id, b);
                }
                Amr::New(new)
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
        f.debug_tuple("Predicate").field(&self.assigned).finish()
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
impl Debug for SpecVar {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        f.debug_tuple("vrID").field(&self.0).finish()
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
