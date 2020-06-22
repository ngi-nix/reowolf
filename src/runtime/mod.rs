mod communication;
mod error;
mod setup2;

#[cfg(test)]
mod my_tests;

use crate::common::*;
use error::*;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum LocalComponentId {
    Native,
    Proto(ProtoComponentId),
}
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Route {
    LocalComponent(LocalComponentId),
    Endpoint { index: usize },
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MyPortInfo {
    polarity: Polarity,
    port: PortId,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Decision {
    Failure,
    Success(Predicate),
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Msg {
    SetupMsg(SetupMsg),
    CommMsg(CommMsg),
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum SetupMsg {
    MyPortInfo(MyPortInfo),
    LeaderEcho { maybe_leader: ControllerId },
    LeaderAnnounce { leader: ControllerId },
    YouAreMyParent,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CommMsg {
    pub round_index: usize,
    pub contents: CommMsgContents,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum CommMsgContents {
    SendPayload(SendPayloadMsg),
    Elaborate { partial_oracle: Predicate }, // SINKWARD
    Failure,                                 // SINKWARD
    Announce { decision: Decision },         // SINKAWAYS
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SendPayloadMsg {
    payload_predicate: Predicate,
    payload: Payload,
}
#[derive(Debug, PartialEq)]
pub enum CommonSatResult {
    FormerNotLatter,
    LatterNotFormer,
    Equivalent,
    New(Predicate),
    Nonexistant,
}
pub struct Endpoint {
    inbox: Vec<u8>,
    stream: TcpStream,
}
#[derive(Debug, Clone)]
pub struct ProtoComponent {
    state: ComponentState,
    ports: HashSet<PortId>,
}
pub trait Logger: Debug {
    fn line_writer(&mut self) -> &mut dyn std::fmt::Write;
    fn dump_log(&self, w: &mut dyn std::io::Write);
}
#[derive(Debug, Clone)]
pub struct EndpointSetup {
    pub sock_addr: SocketAddr,
    pub is_active: bool,
}
#[derive(Debug)]
pub struct EndpointExt {
    endpoint: Endpoint,
    inp_for_emerging_msgs: PortId,
}
#[derive(Debug)]
pub struct Neighborhood {
    parent: Option<usize>,
    children: Vec<usize>, // ordered, deduplicated
}
#[derive(Debug)]
pub struct MemInMsg {
    inp: PortId,
    msg: Payload,
}
#[derive(Debug)]
pub struct IdManager {
    controller_id: ControllerId,
    port_suffix_stream: U32Stream,
    proto_component_suffix_stream: U32Stream,
}
#[derive(Debug)]
pub struct EndpointManager {
    // invariants:
    // 1. endpoint N is registered READ | WRITE with poller
    // 2. Events is empty
    poll: Poll,
    events: Events,
    polled_undrained: IndexSet<usize>,
    delayed_messages: Vec<(usize, Msg)>,
    undelayed_messages: Vec<(usize, Msg)>,
    endpoint_exts: Vec<EndpointExt>,
}
#[derive(Debug, Default)]
pub struct PortInfo {
    polarities: HashMap<PortId, Polarity>,
    peers: HashMap<PortId, PortId>,
    routes: HashMap<PortId, Route>,
}
#[derive(Debug)]
pub struct Connector {
    proto_description: Arc<ProtocolDescription>,
    proto_components: HashMap<ProtoComponentId, ProtoComponent>,
    logger: Box<dyn Logger>,
    id_manager: IdManager,
    native_ports: HashSet<PortId>,
    port_info: PortInfo,
    phased: ConnectorPhased,
}
#[derive(Debug)]
pub enum ConnectorPhased {
    Setup {
        endpoint_setups: Vec<(PortId, EndpointSetup)>,
        surplus_sockets: u16,
    },
    Communication {
        round_index: usize,
        endpoint_manager: EndpointManager,
        neighborhood: Neighborhood,
        mem_inbox: Vec<MemInMsg>,
        native_batches: Vec<NativeBatch>,
        round_result: Result<Option<usize>, SyncError>,
    },
}
#[derive(Debug)]
pub struct StringLogger(ControllerId, String);
#[derive(Default, Debug, Clone, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Predicate {
    pub assigned: BTreeMap<PortId, bool>,
}
#[derive(Debug, Default)]
pub struct NativeBatch {
    // invariant: putters' and getters' polarities respected
    to_put: HashMap<PortId, Payload>,
    to_get: HashSet<PortId>,
}
pub struct MonitoredReader<R: Read> {
    bytes: usize,
    r: R,
}
pub struct NonsyncProtoContext<'a> {
    logger: &'a mut dyn Logger,
    proto_component_id: ProtoComponentId,
    port_info: &'a mut PortInfo,
    id_manager: &'a mut IdManager,
    proto_component_ports: &'a mut HashSet<PortId>,
    unrun_components: &'a mut Vec<(ProtoComponentId, ProtoComponent)>,
}
pub struct SyncProtoContext<'a> {
    logger: &'a mut dyn Logger,
    predicate: &'a Predicate,
    proto_component_id: ProtoComponentId,
    inbox: &'a HashMap<PortId, Payload>,
}

// pub struct MonoPContext<'a> {
//     inner: &'a mut ControllerInner,
//     ports: &'a mut HashSet<PortId>,
//     mono_ps: &'a mut Vec<MonoP>,
// }
// pub struct PolyPContext<'a> {
//     my_subtree_id: SubtreeId,
//     inner: &'a mut Connector,
//     solution_storage: &'a mut SolutionStorage,
// }
// impl PolyPContext<'_> {
//     #[inline(always)]
//     fn reborrow<'a>(&'a mut self) -> PolyPContext<'a> {
//         let Self { solution_storage, my_subtree_id, inner } = self;
//         PolyPContext { solution_storage, my_subtree_id: *my_subtree_id, inner }
//     }
// }
// struct BranchPContext<'m, 'r> {
//     m_ctx: PolyPContext<'m>,
//     ports: &'r HashSet<PortId>,
//     predicate: &'r Predicate,
//     inbox: &'r HashMap<PortId, Payload>,
// }

// #[derive(Debug)]
// pub enum SyncRunResult {
//     BlockingForRecv,
//     AllBranchesComplete,
//     NoBranches,
// }
// #[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
// pub enum PolyId {
//     N,
//     P { index: usize },
// }

// #[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
// pub enum SubtreeId {
//     PolyN,
//     PolyP { index: usize },
//     ChildController { port: PortId },
// }
// #[derive(Debug)]
// pub struct NativeBranch {
//     gotten: HashMap<PortId, Payload>,
//     to_get: HashSet<PortId>,
// }

////////////////
impl PortInfo {
    fn firing_var_for(&self, port: PortId) -> PortId {
        match self.polarities.get(&port).unwrap() {
            Getter => port,
            Putter => *self.peers.get(&port).unwrap(),
        }
    }
}
impl IdManager {
    fn new(controller_id: ControllerId) -> Self {
        Self {
            controller_id,
            port_suffix_stream: Default::default(),
            proto_component_suffix_stream: Default::default(),
        }
    }
    fn new_port_id(&mut self) -> PortId {
        Id { controller_id: self.controller_id, u32_suffix: self.port_suffix_stream.next() }.into()
    }
    fn new_proto_component_id(&mut self) -> ProtoComponentId {
        Id {
            controller_id: self.controller_id,
            u32_suffix: self.proto_component_suffix_stream.next(),
        }
        .into()
    }
}
impl Connector {
    pub fn new_port_pair(&mut self) -> [PortId; 2] {
        // adds two new associated ports, related to each other, and exposed to the native
        let [o, i] = [self.id_manager.new_port_id(), self.id_manager.new_port_id()];
        self.native_ports.insert(o);
        self.native_ports.insert(i);
        // {polarity, peer, route} known. {} unknown.
        self.port_info.polarities.insert(o, Putter);
        self.port_info.polarities.insert(i, Getter);
        self.port_info.peers.insert(o, i);
        self.port_info.peers.insert(i, o);
        let route = Route::LocalComponent(LocalComponentId::Native);
        self.port_info.routes.insert(o, route);
        self.port_info.routes.insert(i, route);
        log!(self.logger, "Added port pair (out->in) {:?} -> {:?}", o, i);
        [o, i]
    }
}
impl EndpointManager {
    fn send_to(&mut self, index: usize, msg: &Msg) -> Result<(), ()> {
        self.endpoint_exts[index].endpoint.send(msg)
    }
    fn try_recv_any(&mut self, deadline: Instant) -> Result<(usize, Msg), TryRecyAnyError> {
        use TryRecyAnyError::*;
        // 1. try messages already buffered
        if let Some(x) = self.undelayed_messages.pop() {
            return Ok(x);
        }
        loop {
            // 2. try read a message from an endpoint that raised an event with poll() but wasn't drained
            while let Some(index) = self.polled_undrained.pop() {
                let endpoint = &mut self.endpoint_exts[index].endpoint;
                if let Some(msg) =
                    endpoint.try_recv().map_err(|error| EndpointError { error, index })?
                {
                    if !endpoint.inbox.is_empty() {
                        // there may be another message waiting!
                        self.polled_undrained.insert(index);
                    }
                    return Ok((index, msg));
                }
            }
            // 3. No message yet. Do we have enough time to poll?
            let remaining = deadline.checked_duration_since(Instant::now()).ok_or(Timeout)?;
            self.poll.poll(&mut self.events, Some(remaining)).map_err(|_| PollFailed)?;
            for event in self.events.iter() {
                let Token(index) = event.token();
                self.polled_undrained.insert(index);
            }
            self.events.clear();
        }
    }
    fn undelay_all(&mut self) {
        if self.undelayed_messages.is_empty() {
            // fast path
            std::mem::swap(&mut self.delayed_messages, &mut self.undelayed_messages);
            return;
        }
        // slow path
        self.undelayed_messages.extend(self.delayed_messages.drain(..));
    }
}
impl Debug for Endpoint {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        f.debug_struct("Endpoint").field("inbox", &self.inbox).finish()
    }
}
impl<R: Read> From<R> for MonitoredReader<R> {
    fn from(r: R) -> Self {
        Self { r, bytes: 0 }
    }
}
impl<R: Read> MonitoredReader<R> {
    pub fn bytes_read(&self) -> usize {
        self.bytes
    }
}
impl<R: Read> Read for MonitoredReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        let n = self.r.read(buf)?;
        self.bytes += n;
        Ok(n)
    }
}
impl Into<Msg> for SetupMsg {
    fn into(self) -> Msg {
        Msg::SetupMsg(self)
    }
}
impl StringLogger {
    pub fn new(controller_id: ControllerId) -> Self {
        Self(controller_id, String::default())
    }
}
impl Logger for StringLogger {
    fn line_writer(&mut self) -> &mut dyn std::fmt::Write {
        use std::fmt::Write;
        let _ = write!(&mut self.1, "\nCID({}): ", self.0);
        self
    }
    fn dump_log(&self, w: &mut dyn std::io::Write) {
        let _ = w.write(self.1.as_bytes());
    }
}
impl std::fmt::Write for StringLogger {
    fn write_str(&mut self, s: &str) -> Result<(), std::fmt::Error> {
        self.1.write_str(s)
    }
}
impl Endpoint {
    fn try_recv<T: serde::de::DeserializeOwned>(&mut self) -> Result<Option<T>, EndpointError> {
        use EndpointError::*;
        // populate inbox as much as possible
        'read_loop: loop {
            match self.stream.read_to_end(&mut self.inbox) {
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break 'read_loop,
                Ok(0) => break 'read_loop,
                Ok(_) => (),
                Err(_e) => return Err(BrokenEndpoint),
            }
        }
        let mut monitored = MonitoredReader::from(&self.inbox[..]);
        match bincode::deserialize_from(&mut monitored) {
            Ok(msg) => {
                let msg_size = monitored.bytes_read();
                self.inbox.drain(0..(msg_size.try_into().unwrap()));
                Ok(Some(msg))
            }
            Err(e) => match *e {
                bincode::ErrorKind::Io(k) if k.kind() == std::io::ErrorKind::UnexpectedEof => {
                    Ok(None)
                }
                _ => Err(MalformedMessage),
                // println!("SERDE ERRKIND {:?}", e);
                // Err(MalformedMessage)
            },
        }
    }
    fn send<T: serde::ser::Serialize>(&mut self, msg: &T) -> Result<(), ()> {
        bincode::serialize_into(&mut self.stream, msg).map_err(drop)
    }
}
impl Connector {
    pub fn get_logger(&self) -> &dyn Logger {
        &*self.logger
    }
    pub fn print_state(&self) {
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        writeln!(
            lock,
            "--- Connector with ControllerId={:?}.\n::LOG_DUMP:\n",
            self.id_manager.controller_id
        )
        .unwrap();
        self.get_logger().dump_log(&mut lock);
        writeln!(lock, "\n\nDEBUG_PRINT:\n{:#?}\n", self).unwrap();
    }
}
// impl Debug for SolutionStorage {
//     fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
//         f.pad("Solutions: [")?;
//         for (subtree_id, &index) in self.subtree_id_to_index.iter() {
//             let sols = &self.subtree_solutions[index];
//             f.write_fmt(format_args!("{:?}: {:?}, ", subtree_id, sols))?;
//         }
//         f.pad("]")
//     }
// }

impl Predicate {
    #[inline]
    pub fn inserted(mut self, k: PortId, v: bool) -> Self {
        self.assigned.insert(k, v);
        self
    }
    // returns true IFF self.unify would return Equivalent OR FormerNotLatter
    pub fn satisfies(&self, other: &Self) -> bool {
        let mut s_it = self.assigned.iter();
        let mut s = if let Some(s) = s_it.next() {
            s
        } else {
            return other.assigned.is_empty();
        };
        for (oid, ob) in other.assigned.iter() {
            while s.0 < oid {
                s = if let Some(s) = s_it.next() {
                    s
                } else {
                    return false;
                };
            }
            if s.0 > oid || s.1 != ob {
                return false;
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
    pub fn common_satisfier(&self, other: &Self) -> CommonSatResult {
        use CommonSatResult::*;
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
                        return Nonexistant;
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
            [true, true] => Equivalent,       // ... equivalent to both.
            [false, true] => FormerNotLatter, // ... equivalent to self.
            [true, false] => LatterNotFormer, // ... equivalent to other.
            [false, false] => {
                // ... which is the union of the predicates' assignments but
                //     is equivalent to neither self nor other.
                let mut new = self.clone();
                for (&id, &b) in o_not_s {
                    new.assigned.insert(id, b);
                }
                New(new)
            }
        }
    }

    pub fn iter_matching(&self, value: bool) -> impl Iterator<Item = PortId> + '_ {
        self.assigned
            .iter()
            .filter_map(move |(&channel_id, &b)| if b == value { Some(channel_id) } else { None })
    }

    pub fn batch_assign_nones(&mut self, channel_ids: impl Iterator<Item = PortId>, value: bool) {
        for channel_id in channel_ids {
            self.assigned.entry(channel_id).or_insert(value);
        }
    }
    // pub fn replace_assignment(&mut self, channel_id: PortId, value: bool) -> Option<bool> {
    //     self.assigned.insert(channel_id, value)
    // }
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
    pub fn query(&self, x: PortId) -> Option<bool> {
        self.assigned.get(&x).copied()
    }
}
