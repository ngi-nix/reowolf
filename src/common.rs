///////////////////// PRELUDE /////////////////////

pub use core::{
    cmp::Ordering,
    fmt::{Debug, Formatter},
    hash::{Hash, Hasher},
    ops::{Range, RangeFrom},
    time::Duration,
};
pub use indexmap::{IndexMap, IndexSet};
pub use maplit::{hashmap, hashset};
pub use mio::{
    net::{TcpListener, TcpStream},
    Event, Evented, Events, Poll, PollOpt, Ready, Token,
};
pub use std::{
    collections::{hash_map::Entry, BTreeMap, HashMap, HashSet},
    convert::TryInto,
    net::SocketAddr,
    sync::Arc,
    time::Instant,
};
pub use Polarity::*;

///////////////////// DEFS /////////////////////

pub type ControllerId = u32;
pub type ChannelIndex = u32;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, Hash, PartialOrd)]
pub struct PortId {
    pub(crate) controller_id: ControllerId,
    pub(crate) port_index: u32,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Payload(Arc<Vec<u8>>);

/// This is a unique identifier for a channel (i.e., port).
#[derive(Debug, Eq, PartialEq, Clone, Hash, Copy, Ord, PartialOrd)]
pub struct ChannelId {
    pub(crate) controller_id: ControllerId,
    pub(crate) channel_index: ChannelIndex,
}

#[derive(Debug, Eq, PartialEq, Clone, Hash, Copy, Ord, PartialOrd)]
pub enum Polarity {
    Putter, // output port (from the perspective of the component)
    Getter, // input port (from the perspective of the component)
}

#[derive(
    Eq, PartialEq, Ord, PartialOrd, Hash, Copy, Clone, serde::Serialize, serde::Deserialize,
)]
#[repr(C)]
pub struct Port(pub u32); // ports are COPY

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum MainComponentErr {
    NoSuchComponent,
    NonPortTypeParameters,
    CannotMovePort(Port),
    WrongNumberOfParamaters { expected: usize },
    UnknownPort(Port),
    WrongPortPolarity { param_index: usize, port: Port },
    DuplicateMovedPort(Port),
}
pub trait ProtocolDescription: Sized {
    type S: ComponentState<D = Self>;

    fn parse(pdl: &[u8]) -> Result<Self, String>;
    fn component_polarities(&self, identifier: &[u8]) -> Result<Vec<Polarity>, MainComponentErr>;
    fn new_main_component(&self, identifier: &[u8], ports: &[Port]) -> Self::S;
}

pub trait ComponentState: Sized + Clone {
    type D: ProtocolDescription;
    fn pre_sync_run<C: MonoContext<D = Self::D, S = Self>>(
        &mut self,
        runtime_ctx: &mut C,
        protocol_description: &Self::D,
    ) -> MonoBlocker;

    fn sync_run<C: PolyContext<D = Self::D>>(
        &mut self,
        runtime_ctx: &mut C,
        protocol_description: &Self::D,
    ) -> PolyBlocker;
}

#[derive(Debug, Clone)]
pub enum MonoBlocker {
    Inconsistent,
    ComponentExit,
    SyncBlockStart,
}

#[derive(Debug, Clone)]
pub enum PolyBlocker {
    Inconsistent,
    SyncBlockEnd,
    CouldntReadMsg(Port),
    CouldntCheckFiring(Port),
    PutMsg(Port, Payload),
}

pub trait MonoContext {
    type D: ProtocolDescription;
    type S: ComponentState<D = Self::D>;

    fn new_component(&mut self, moved_ports: HashSet<Port>, init_state: Self::S);
    fn new_channel(&mut self) -> [Port; 2];
    fn new_random(&mut self) -> u64;
}
pub trait PolyContext {
    type D: ProtocolDescription;

    fn is_firing(&mut self, port: Port) -> Option<bool>;
    fn read_msg(&mut self, port: Port) -> Option<&Payload>;
}

///////////////////// IMPL /////////////////////
impl Payload {
    pub fn new(len: usize) -> Payload {
        let mut v = Vec::with_capacity(len);
        unsafe {
            v.set_len(len);
        }
        Payload(Arc::new(v))
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        Arc::make_mut(&mut self.0) as _
    }
    pub fn concat_with(&mut self, other: &Self) {
        let bytes = other.as_slice().iter().copied();
        let me = Arc::make_mut(&mut self.0);
        me.extend(bytes);
    }
}
impl serde::Serialize for Payload {
    fn serialize<S>(
        &self,
        serializer: S,
    ) -> std::result::Result<<S as serde::Serializer>::Ok, <S as serde::Serializer>::Error>
    where
        S: serde::Serializer,
    {
        let inner: &Vec<u8> = &self.0;
        inner.serialize(serializer)
    }
}
impl<'de> serde::Deserialize<'de> for Payload {
    fn deserialize<D>(
        deserializer: D,
    ) -> std::result::Result<Self, <D as serde::Deserializer<'de>>::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let inner: Vec<u8> = Vec::deserialize(deserializer)?;
        Ok(Self(Arc::new(inner)))
    }
}
impl std::iter::FromIterator<u8> for Payload {
    fn from_iter<I: IntoIterator<Item = u8>>(it: I) -> Self {
        Self(Arc::new(it.into_iter().collect()))
    }
}
impl From<Vec<u8>> for Payload {
    fn from(s: Vec<u8>) -> Self {
        Self(s.into())
    }
}
impl Debug for Port {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "Port({})", self.0)
    }
}
impl Port {
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
    pub fn to_raw(self) -> u32 {
        self.0
    }
    pub fn to_token(self) -> mio::Token {
        mio::Token(self.0.try_into().unwrap())
    }
    pub fn from_token(t: mio::Token) -> Self {
        Self(t.0.try_into().unwrap())
    }
}
