// #[cfg(feature = "ffi")]
// pub mod ffi;

// mod actors;
// pub(crate) mod communication;
// pub(crate) mod connector;
// pub(crate) mod endpoint;
// pub mod errors;
// mod serde;
mod my_tests;
mod setup2;
// pub(crate) mod setup;
// mod v2;

use crate::common::*;
// use actors::*;
// use endpoint::*;
// use errors::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Decision {
    Failure,
    Success(Predicate),
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum Msg {
    SetupMsg(SetupMsg),
    CommMsg(CommMsg),
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MyPortInfo {
    polarity: Polarity,
    port: PortId,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum SetupMsg {
    // sent by the passive endpoint to the active endpoint
    // MyPortInfo(MyPortInfo),
    LeaderEcho { maybe_leader: ControllerId },
    LeaderAnnounce { leader: ControllerId },
    YouAreMyParent,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct CommMsg {
    pub round_index: usize,
    pub contents: CommMsgContents,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum CommMsgContents {
    SendPayload { payload_predicate: Predicate, payload: Payload },
    Elaborate { partial_oracle: Predicate }, // SINKWARD
    Failure,                                 // SINKWARD
    Announce { decision: Decision },         // SINKAWAYS
}
#[derive(Debug, PartialEq)]
pub(crate) enum CommonSatResult {
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
#[derive(Debug, Default)]
pub struct IntStream {
    next: u32,
}
#[derive(Debug)]
pub struct IdManager {
    controller_id: ControllerId,
    port_suffix_stream: IntStream,
}
#[derive(Debug)]
pub struct ProtoComponent {
    state: ComponentState,
    ports: HashSet<PortId>,
}
#[derive(Debug)]
pub enum InpRoute {
    NativeComponent,
    ProtoComponent { index: usize },
    Endpoint { index: usize },
}
pub trait Logger: Debug {
    fn line_writer(&mut self) -> &mut dyn std::fmt::Write;
    fn dump_log(&self, w: &mut dyn std::io::Write);
}
#[derive(Debug, Clone)]
pub struct EndpointSetup {
    pub polarity: Polarity,
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
pub struct EndpointPoller {
    poll: Poll,
    events: Events,
    undrained_endpoints: IndexSet<usize>,
    delayed_messages: Vec<(usize, Msg)>,
    undelayed_messages: Vec<(usize, Msg)>,
}
#[derive(Debug)]
pub struct Connector {
    logger: Box<dyn Logger>,
    proto_description: Arc<ProtocolDescription>,
    id_manager: IdManager,
    native_ports: HashSet<PortId>,
    proto_components: Vec<ProtoComponent>,
    outp_to_inp: HashMap<PortId, PortId>,
    inp_to_route: HashMap<PortId, InpRoute>,
    phased: ConnectorPhased,
}
#[derive(Debug)]
pub enum ConnectorPhased {
    Setup {
        endpoint_setups: Vec<(PortId, EndpointSetup)>,
        surplus_sockets: u16,
    },
    Communication {
        endpoint_poller: EndpointPoller,
        endpoint_exts: Vec<EndpointExt>,
        neighborhood: Neighborhood,
        mem_inbox: Vec<MemInMsg>,
    },
}
#[derive(Debug)]
pub struct StringLogger(ControllerId, String);
#[derive(Clone, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct Predicate {
    pub assigned: BTreeMap<PortId, bool>,
}
#[derive(Debug, Default)]
struct SyncBatch {
    puts: HashMap<PortId, Payload>,
    gets: HashSet<PortId>,
}
pub struct MonitoredReader<R: Read> {
    bytes: usize,
    r: R,
}
pub enum EndpointRecvErr {
    MalformedMessage,
    BrokenEndpoint,
}
pub struct SyncContext<'a> {
    connector: &'a mut Connector,
}
pub struct NonsyncContext<'a> {
    connector: &'a mut Connector,
}
enum TryRecyAnyError {
    Timeout,
    PollFailed,
    EndpointRecvErr { error: EndpointRecvErr, index: usize },
    BrokenEndpoint(usize),
}
////////////////
impl EndpointPoller {
    fn try_recv_any(
        &mut self,
        endpoint_exts: &mut [EndpointExt],
        deadline: Instant,
    ) -> Result<(usize, Msg), TryRecyAnyError> {
        use TryRecyAnyError::*;
        // 1. try messages already buffered
        if let Some(x) = self.undelayed_messages.pop() {
            return Ok(x);
        }
        // 2. try read from sockets nonblocking
        while let Some(index) = self.undrained_endpoints.pop() {
            if let Some(msg) = endpoint_exts[index]
                .endpoint
                .try_recv()
                .map_err(|error| EndpointRecvErr { error, index })?
            {
                return Ok((index, msg));
            }
        }
        // 3. poll for progress
        loop {
            let remaining = deadline.checked_duration_since(Instant::now()).ok_or(Timeout)?;
            self.poll.poll(&mut self.events, Some(remaining)).map_err(|_| PollFailed)?;
            for event in self.events.iter() {
                let Token(index) = event.token();
                if let Some(msg) = endpoint_exts[index]
                    .endpoint
                    .try_recv()
                    .map_err(|error| EndpointRecvErr { error, index })?
                {
                    return Ok((index, msg));
                }
            }
        }
    }
    fn undelay_all(&mut self) {
        self.undelayed_messages.extend(self.delayed_messages.drain(..));
    }
}
impl Debug for Endpoint {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        f.debug_struct("Endpoint").field("inbox", &self.inbox).finish()
    }
}
impl NonsyncContext<'_> {
    pub fn new_component(&mut self, moved_ports: HashSet<PortId>, init_state: ComponentState) {
        todo!()
    }
    pub fn new_channel(&mut self) -> [PortId; 2] {
        todo!()
    }
}
impl SyncContext<'_> {
    pub fn is_firing(&mut self, port: PortId) -> Option<bool> {
        todo!()
    }
    pub fn read_msg(&mut self, port: PortId) -> Option<&Payload> {
        todo!()
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
impl Debug for Predicate {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        f.pad("{")?;
        for (port, &v) in self.assigned.iter() {
            f.write_fmt(format_args!("{:?}=>{}, ", port, if v { 'T' } else { 'F' }))?
        }
        f.pad("}")
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
impl IntStream {
    fn next(&mut self) -> u32 {
        if self.next == u32::MAX {
            panic!("NO NEXT!")
        }
        self.next += 1;
        self.next - 1
    }
}
impl IdManager {
    fn next_port(&mut self) -> PortId {
        let port_suffix = self.port_suffix_stream.next();
        let controller_id = self.controller_id;
        PortId { controller_id, port_index: port_suffix }
    }
    fn new(controller_id: ControllerId) -> Self {
        Self { controller_id, port_suffix_stream: Default::default() }
    }
}
impl Endpoint {
    fn try_recv<T: serde::de::DeserializeOwned>(&mut self) -> Result<Option<T>, EndpointRecvErr> {
        use EndpointRecvErr::*;
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
        writeln!(lock, "DEBUG_PRINT:\n{:#?}\n", self).unwrap();
    }
}

// #[derive(Debug)]
// pub enum Connector {
//     Unconfigured(Unconfigured),
//     Configured(Configured),
//     Connected(Connected), // TODO consider boxing. currently takes up a lot of stack space
// }
// #[derive(Debug)]
// pub struct Unconfigured {
//     pub controller_id: ControllerId,
// }
// #[derive(Debug)]
// pub struct Configured {
//     controller_id: ControllerId,
//     polarities: Vec<Polarity>,
//     bindings: HashMap<usize, PortBinding>,
//     protocol_description: Arc<ProtocolD>,
//     main_component: Vec<u8>,
//     logger: String,
// }
// #[derive(Debug)]
// pub struct Connected {
//     native_interface: Vec<(PortId, Polarity)>,
//     sync_batches: Vec<SyncBatch>,
//     // controller is cooperatively scheduled with the native application
//     // (except for transport layer behind Endpoints, which are managed by the OS)
//     // control flow is passed to the controller during methods on Connector (primarily, connect and sync).
//     controller: Controller,
// }

// #[derive(Debug, Copy, Clone)]
// pub enum PortBinding {
//     Native,
//     Active(SocketAddr),
//     Passive(SocketAddr),
// }

// #[derive(Debug)]
// struct Arena<T> {
//     storage: Vec<T>,
// }

// #[derive(Debug)]
// struct ReceivedMsg {
//     recipient: PortId,
//     msg: Msg,
// }

// #[derive(Debug)]
// struct MessengerState {
//     poll: Poll,
//     events: Events,
//     delayed: Vec<ReceivedMsg>,
//     undelayed: Vec<ReceivedMsg>,
//     polled_undrained: IndexSet<PortId>,
// }
// #[derive(Debug)]
// struct ChannelIdStream {
//     controller_id: ControllerId,
//     next_channel_index: ChannelIndex,
// }

// #[derive(Debug)]
// struct Controller {
//     protocol_description: Arc<ProtocolD>,
//     inner: ControllerInner,
//     ephemeral: ControllerEphemeral,
//     unrecoverable_error: Option<SyncErr>, // prevents future calls to Sync
// }
// #[derive(Debug)]
// struct ControllerInner {
//     round_index: usize,
//     channel_id_stream: ChannelIdStream,
//     endpoint_exts: Arena<EndpointExt>,
//     messenger_state: MessengerState,
//     mono_n: MonoN,       // state at next round start
//     mono_ps: Vec<MonoP>, // state at next round start
//     family: ControllerFamily,
//     logger: String,
// }

// /// This structure has its state entirely reset between synchronous rounds
// #[derive(Debug, Default)]
// struct ControllerEphemeral {
//     solution_storage: SolutionStorage,
//     poly_n: Option<PolyN>,
//     poly_ps: Vec<PolyP>,
//     mono_ps: Vec<MonoP>,
//     port_to_holder: HashMap<PortId, PolyId>,
// }

// #[derive(Debug)]
// struct ControllerFamily {
//     parent_port: Option<PortId>,
//     children_ports: Vec<PortId>,
// }

// #[derive(Debug)]
// pub(crate) enum SyncRunResult {
//     BlockingForRecv,
//     AllBranchesComplete,
//     NoBranches,
// }

// // Used to identify poly actors
// #[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
// enum PolyId {
//     N,
//     P { index: usize },
// }

// #[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
// pub(crate) enum SubtreeId {
//     PolyN,
//     PolyP { index: usize },
//     ChildController { port: PortId },
// }

// pub(crate) struct MonoPContext<'a> {
//     inner: &'a mut ControllerInner,
//     ports: &'a mut HashSet<PortId>,
//     mono_ps: &'a mut Vec<MonoP>,
// }
// pub(crate) struct PolyPContext<'a> {
//     my_subtree_id: SubtreeId,
//     inner: &'a mut ControllerInner,
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

// #[derive(Default)]
// pub(crate) struct SolutionStorage {
//     old_local: HashSet<Predicate>,
//     new_local: HashSet<Predicate>,
//     // this pair acts as SubtreeId -> HashSet<Predicate> which is friendlier to iteration
//     subtree_solutions: Vec<HashSet<Predicate>>,
//     subtree_id_to_index: HashMap<SubtreeId, usize>,
// }

// trait Messengerlike {
//     fn get_state_mut(&mut self) -> &mut MessengerState;
//     fn get_endpoint_mut(&mut self, eport: PortId) -> &mut Endpoint;

//     fn delay(&mut self, received: ReceivedMsg) {
//         self.get_state_mut().delayed.push(received);
//     }
//     fn undelay_all(&mut self) {
//         let MessengerState { delayed, undelayed, .. } = self.get_state_mut();
//         undelayed.extend(delayed.drain(..))
//     }

//     fn send(&mut self, to: PortId, msg: Msg) -> Result<(), EndpointErr> {
//         self.get_endpoint_mut(to).send(msg)
//     }

//     // attempt to receive a message from one of the endpoints before the deadline
//     fn recv(&mut self, deadline: Instant) -> Result<Option<ReceivedMsg>, MessengerRecvErr> {
//         // try get something buffered
//         if let Some(x) = self.get_state_mut().undelayed.pop() {
//             return Ok(Some(x));
//         }

//         loop {
//             // polled_undrained may not be empty
//             while let Some(eport) = self.get_state_mut().polled_undrained.pop() {
//                 if let Some(msg) = self
//                     .get_endpoint_mut(eport)
//                     .recv()
//                     .map_err(|e| MessengerRecvErr::EndpointErr(eport, e))?
//                 {
//                     // this endpoint MAY still have messages! check again in future
//                     self.get_state_mut().polled_undrained.insert(eport);
//                     return Ok(Some(ReceivedMsg { recipient: eport, msg }));
//                 }
//             }

//             let state = self.get_state_mut();
//             match state.poll_events(deadline) {
//                 Ok(()) => {
//                     for e in state.events.iter() {
//                         state.polled_undrained.insert(PortId::from_token(e.token()));
//                     }
//                 }
//                 Err(PollDeadlineErr::PollingFailed) => return Err(MessengerRecvErr::PollingFailed),
//                 Err(PollDeadlineErr::Timeout) => return Ok(None),
//             }
//         }
//     }
//     fn recv_blocking(&mut self) -> Result<ReceivedMsg, MessengerRecvErr> {
//         // try get something buffered
//         if let Some(x) = self.get_state_mut().undelayed.pop() {
//             return Ok(x);
//         }

//         loop {
//             // polled_undrained may not be empty
//             while let Some(eport) = self.get_state_mut().polled_undrained.pop() {
//                 if let Some(msg) = self
//                     .get_endpoint_mut(eport)
//                     .recv()
//                     .map_err(|e| MessengerRecvErr::EndpointErr(eport, e))?
//                 {
//                     // this endpoint MAY still have messages! check again in future
//                     self.get_state_mut().polled_undrained.insert(eport);
//                     return Ok(ReceivedMsg { recipient: eport, msg });
//                 }
//             }

//             let state = self.get_state_mut();

//             state
//                 .poll
//                 .poll(&mut state.events, None)
//                 .map_err(|_| MessengerRecvErr::PollingFailed)?;
//             for e in state.events.iter() {
//                 state.polled_undrained.insert(PortId::from_token(e.token()));
//             }
//         }
//     }
// }

// /////////////////////////////////
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
// impl From<EvalErr> for SyncErr {
//     fn from(e: EvalErr) -> SyncErr {
//         SyncErr::EvalErr(e)
//     }
// }
// impl From<MessengerRecvErr> for SyncErr {
//     fn from(e: MessengerRecvErr) -> SyncErr {
//         SyncErr::MessengerRecvErr(e)
//     }
// }
// impl From<MessengerRecvErr> for ConnectErr {
//     fn from(e: MessengerRecvErr) -> ConnectErr {
//         ConnectErr::MessengerRecvErr(e)
//     }
// }
// impl<T> Default for Arena<T> {
//     fn default() -> Self {
//         Self { storage: vec![] }
//     }
// }
// impl<T> Arena<T> {
//     pub fn alloc(&mut self, t: T) -> PortId {
//         self.storage.push(t);
//         let l: u32 = self.storage.len().try_into().unwrap();
//         PortId::from_raw(l - 1u32)
//     }
//     pub fn get(&self, key: PortId) -> Option<&T> {
//         self.storage.get(key.to_raw() as usize)
//     }
//     pub fn get_mut(&mut self, key: PortId) -> Option<&mut T> {
//         self.storage.get_mut(key.to_raw() as usize)
//     }
//     pub fn type_convert<X>(self, f: impl FnMut((PortId, T)) -> X) -> Arena<X> {
//         Arena { storage: self.keyspace().zip(self.storage.into_iter()).map(f).collect() }
//     }
//     pub fn iter(&self) -> impl Iterator<Item = (PortId, &T)> {
//         self.keyspace().zip(self.storage.iter())
//     }
//     pub fn len(&self) -> usize {
//         self.storage.len()
//     }
//     pub fn keyspace(&self) -> impl Iterator<Item = PortId> {
//         (0u32..self.storage.len().try_into().unwrap()).map(PortId::from_raw)
//     }
// }

// impl ChannelIdStream {
//     fn new(controller_id: ControllerId) -> Self {
//         Self { controller_id, next_channel_index: 0 }
//     }
//     fn next(&mut self) -> ChannelId {
//         self.next_channel_index += 1;
//         ChannelId { controller_id: self.controller_id, channel_index: self.next_channel_index - 1 }
//     }
// }

// impl MessengerState {
//     // does NOT guarantee that events is non-empty
//     fn poll_events(&mut self, deadline: Instant) -> Result<(), PollDeadlineErr> {
//         use PollDeadlineErr::*;
//         self.events.clear();
//         let poll_timeout = deadline.checked_duration_since(Instant::now()).ok_or(Timeout)?;
//         self.poll.poll(&mut self.events, Some(poll_timeout)).map_err(|_| PollingFailed)?;
//         Ok(())
//     }
// }
// impl From<PollDeadlineErr> for ConnectErr {
//     fn from(e: PollDeadlineErr) -> ConnectErr {
//         match e {
//             PollDeadlineErr::Timeout => ConnectErr::Timeout,
//             PollDeadlineErr::PollingFailed => ConnectErr::PollingFailed,
//         }
//     }
// }

// impl std::ops::Not for Polarity {
//     type Output = Self;
//     fn not(self) -> Self::Output {
//         use Polarity::*;
//         match self {
//             Putter => Getter,
//             Getter => Putter,
//         }
//     }
// }

// impl Predicate {
//     // returns true IFF self.unify would return Equivalent OR FormerNotLatter
//     pub fn satisfies(&self, other: &Self) -> bool {
//         let mut s_it = self.assigned.iter();
//         let mut s = if let Some(s) = s_it.next() {
//             s
//         } else {
//             return other.assigned.is_empty();
//         };
//         for (oid, ob) in other.assigned.iter() {
//             while s.0 < oid {
//                 s = if let Some(s) = s_it.next() {
//                     s
//                 } else {
//                     return false;
//                 };
//             }
//             if s.0 > oid || s.1 != ob {
//                 return false;
//             }
//         }
//         true
//     }

//     /// Given self and other, two predicates, return the most general Predicate possible, N
//     /// such that n.satisfies(self) && n.satisfies(other).
//     /// If none exists Nonexistant is returned.
//     /// If the resulting predicate is equivlanet to self, other, or both,
//     /// FormerNotLatter, LatterNotFormer and Equivalent are returned respectively.
//     /// otherwise New(N) is returned.
//     pub fn common_satisfier(&self, other: &Self) -> CommonSatResult {
//         use CommonSatResult::*;
//         // iterators over assignments of both predicates. Rely on SORTED ordering of BTreeMap's keys.
//         let [mut s_it, mut o_it] = [self.assigned.iter(), other.assigned.iter()];
//         let [mut s, mut o] = [s_it.next(), o_it.next()];
//         // lists of assignments in self but not other and vice versa.
//         let [mut s_not_o, mut o_not_s] = [vec![], vec![]];
//         loop {
//             match [s, o] {
//                 [None, None] => break,
//                 [None, Some(x)] => {
//                     o_not_s.push(x);
//                     o_not_s.extend(o_it);
//                     break;
//                 }
//                 [Some(x), None] => {
//                     s_not_o.push(x);
//                     s_not_o.extend(s_it);
//                     break;
//                 }
//                 [Some((sid, sb)), Some((oid, ob))] => {
//                     if sid < oid {
//                         // o is missing this element
//                         s_not_o.push((sid, sb));
//                         s = s_it.next();
//                     } else if sid > oid {
//                         // s is missing this element
//                         o_not_s.push((oid, ob));
//                         o = o_it.next();
//                     } else if sb != ob {
//                         assert_eq!(sid, oid);
//                         // both predicates assign the variable but differ on the value
//                         return Nonexistant;
//                     } else {
//                         // both predicates assign the variable to the same value
//                         s = s_it.next();
//                         o = o_it.next();
//                     }
//                 }
//             }
//         }
//         // Observed zero inconsistencies. A unified predicate exists...
//         match [s_not_o.is_empty(), o_not_s.is_empty()] {
//             [true, true] => Equivalent,       // ... equivalent to both.
//             [false, true] => FormerNotLatter, // ... equivalent to self.
//             [true, false] => LatterNotFormer, // ... equivalent to other.
//             [false, false] => {
//                 // ... which is the union of the predicates' assignments but
//                 //     is equivalent to neither self nor other.
//                 let mut new = self.clone();
//                 for (&id, &b) in o_not_s {
//                     new.assigned.insert(id, b);
//                 }
//                 New(new)
//             }
//         }
//     }

//     pub fn iter_matching(&self, value: bool) -> impl Iterator<Item = ChannelId> + '_ {
//         self.assigned
//             .iter()
//             .filter_map(move |(&channel_id, &b)| if b == value { Some(channel_id) } else { None })
//     }

//     pub fn batch_assign_nones(
//         &mut self,
//         channel_ids: impl Iterator<Item = ChannelId>,
//         value: bool,
//     ) {
//         for channel_id in channel_ids {
//             self.assigned.entry(channel_id).or_insert(value);
//         }
//     }
//     pub fn replace_assignment(&mut self, channel_id: ChannelId, value: bool) -> Option<bool> {
//         self.assigned.insert(channel_id, value)
//     }
//     pub fn union_with(&self, other: &Self) -> Option<Self> {
//         let mut res = self.clone();
//         for (&channel_id, &assignment_1) in other.assigned.iter() {
//             match res.assigned.insert(channel_id, assignment_1) {
//                 Some(assignment_2) if assignment_1 != assignment_2 => return None,
//                 _ => {}
//             }
//         }
//         Some(res)
//     }
//     pub fn query(&self, x: ChannelId) -> Option<bool> {
//         self.assigned.get(&x).copied()
//     }
//     pub fn new_trivial() -> Self {
//         Self { assigned: Default::default() }
//     }
// }

// #[test]
// fn pred_sat() {
//     use maplit::btreemap;
//     let mut c = ChannelIdStream::new(0);
//     let ch = std::iter::repeat_with(move || c.next()).take(5).collect::<Vec<_>>();
//     let p = Predicate::new_trivial();
//     let p_0t = Predicate { assigned: btreemap! { ch[0] => true } };
//     let p_0f = Predicate { assigned: btreemap! { ch[0] => false } };
//     let p_0f_3f = Predicate { assigned: btreemap! { ch[0] => false, ch[3] => false } };
//     let p_0f_3t = Predicate { assigned: btreemap! { ch[0] => false, ch[3] => true } };

//     assert!(p.satisfies(&p));
//     assert!(p_0t.satisfies(&p_0t));
//     assert!(p_0f.satisfies(&p_0f));
//     assert!(p_0f_3f.satisfies(&p_0f_3f));
//     assert!(p_0f_3t.satisfies(&p_0f_3t));

//     assert!(p_0t.satisfies(&p));
//     assert!(p_0f.satisfies(&p));
//     assert!(p_0f_3f.satisfies(&p_0f));
//     assert!(p_0f_3t.satisfies(&p_0f));

//     assert!(!p.satisfies(&p_0t));
//     assert!(!p.satisfies(&p_0f));
//     assert!(!p_0f.satisfies(&p_0t));
//     assert!(!p_0t.satisfies(&p_0f));
//     assert!(!p_0f_3f.satisfies(&p_0f_3t));
//     assert!(!p_0f_3t.satisfies(&p_0f_3f));
//     assert!(!p_0t.satisfies(&p_0f_3f));
//     assert!(!p_0f.satisfies(&p_0f_3f));
//     assert!(!p_0t.satisfies(&p_0f_3t));
//     assert!(!p_0f.satisfies(&p_0f_3t));
// }

// #[test]
// fn pred_common_sat() {
//     use maplit::btreemap;
//     use CommonSatResult::*;

//     let mut c = ChannelIdStream::new(0);
//     let ch = std::iter::repeat_with(move || c.next()).take(5).collect::<Vec<_>>();
//     let p = Predicate::new_trivial();
//     let p_0t = Predicate { assigned: btreemap! { ch[0] => true } };
//     let p_0f = Predicate { assigned: btreemap! { ch[0] => false } };
//     let p_3f = Predicate { assigned: btreemap! { ch[3] => false } };
//     let p_0f_3f = Predicate { assigned: btreemap! { ch[0] => false, ch[3] => false } };
//     let p_0f_3t = Predicate { assigned: btreemap! { ch[0] => false, ch[3] => true } };

//     assert_eq![p.common_satisfier(&p), Equivalent];
//     assert_eq![p_0t.common_satisfier(&p_0t), Equivalent];

//     assert_eq![p.common_satisfier(&p_0t), LatterNotFormer];
//     assert_eq![p_0t.common_satisfier(&p), FormerNotLatter];

//     assert_eq![p_0t.common_satisfier(&p_0f), Nonexistant];
//     assert_eq![p_0f_3t.common_satisfier(&p_0f_3f), Nonexistant];
//     assert_eq![p_0f_3t.common_satisfier(&p_3f), Nonexistant];
//     assert_eq![p_3f.common_satisfier(&p_0f_3t), Nonexistant];

//     assert_eq![p_0f.common_satisfier(&p_3f), New(p_0f_3f)];
// }
