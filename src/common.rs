///////////////////// PRELUDE /////////////////////

pub use crate::protocol::{ComponentState, ProtocolDescription};
pub use crate::runtime::{NonsyncContext, SyncContext};

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
    io::{Read, Write},
    net::SocketAddr,
    sync::Arc,
    time::Instant,
};
pub use Polarity::*;

///////////////////// DEFS /////////////////////

pub type ControllerId = u32;
pub type PortSuffix = u32;

// globally unique
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, Hash, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub struct PortId {
    pub(crate) controller_id: ControllerId,
    pub(crate) port_index: PortSuffix,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Payload(Arc<Vec<u8>>);

#[derive(
    Debug, Eq, PartialEq, Clone, Hash, Copy, Ord, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub enum Polarity {
    Putter, // output port (from the perspective of the component)
    Getter, // input port (from the perspective of the component)
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum AddComponentError {
    NoSuchComponent,
    NonPortTypeParameters,
    CannotMovePort(PortId),
    WrongNumberOfParamaters { expected: usize },
    UnknownPort(PortId),
    WrongPortPolarity { port: PortId, expected_polarity: Polarity },
    DuplicateMovedPort(PortId),
}

#[derive(Debug, Clone)]
pub enum NonsyncBlocker {
    Inconsistent,
    ComponentExit,
    SyncBlockStart,
}

#[derive(Debug, Clone)]
pub enum SyncBlocker {
    Inconsistent,
    SyncBlockEnd,
    CouldntReadMsg(PortId),
    CouldntCheckFiring(PortId),
    PutMsg(PortId, Payload),
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
impl Debug for PortId {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "PortId({},{})", self.controller_id, self.port_index)
    }
}
