///////////////////// PRELUDE /////////////////////
pub(crate) use crate::protocol::{ComponentState, ProtocolDescription};
pub(crate) use crate::runtime::{error::AddComponentError, NonsyncProtoContext, SyncProtoContext};
pub(crate) use core::{
    cmp::Ordering,
    fmt::{Debug, Formatter},
    hash::Hash,
    ops::Range,
    time::Duration,
};
pub(crate) use maplit::hashmap;
pub(crate) use mio::{
    net::{TcpListener, TcpStream},
    Events, Interest, Poll, Token,
};
pub(crate) use std::{
    collections::{BTreeMap, HashMap, HashSet},
    convert::TryInto,
    io::{Read, Write},
    net::SocketAddr,
    sync::Arc,
    time::Instant,
};
pub(crate) use Polarity::*;

pub(crate) trait IdParts {
    fn id_parts(self) -> (ConnectorId, U32Suffix);
}

/// Used by various distributed algorithms to identify connectors.
pub type ConnectorId = u32;

/// Used in conjunction with the `ConnectorId` type to create identifiers for ports and components
pub type U32Suffix = u32;
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, Hash, PartialOrd, serde::Serialize, serde::Deserialize,
)]

/// Generalization of a port/component identifier
#[repr(C)]
pub struct Id {
    pub(crate) connector_id: ConnectorId,
    pub(crate) u32_suffix: U32Suffix,
}
#[derive(Clone, Debug, Default)]
pub struct U32Stream {
    next: u32,
}

/// Identifier of a component in a session
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, Hash, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub struct ComponentId(Id); // PUB because it can be returned by errors

/// Identifier of a port in a session
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, Hash, PartialOrd, serde::Serialize, serde::Deserialize,
)]
#[repr(transparent)]
pub struct PortId(Id);

/// A safely aliasable heap-allocated payload of message bytes
#[derive(Default, Eq, PartialEq, Clone, Ord, PartialOrd)]
pub struct Payload(Arc<Vec<u8>>);
#[derive(
    Debug, Eq, PartialEq, Clone, Hash, Copy, Ord, PartialOrd, serde::Serialize, serde::Deserialize,
)]

/// "Orientation" of a port, determining whether they can send or receive messages with `put` and `get` respectively.
#[repr(C)]
pub enum Polarity {
    Putter, // output port (from the perspective of the component)
    Getter, // input port (from the perspective of the component)
}
#[derive(
    Debug, Eq, PartialEq, Clone, Hash, Copy, Ord, PartialOrd, serde::Serialize, serde::Deserialize,
)]

/// "Orientation" of a transport-layer network endpoint, dictating how it's connection procedure should
/// be conducted. Corresponds with connect() / accept() familiar to TCP socket programming.
#[repr(C)]
pub enum EndpointPolarity {
    Active,  // calls connect()
    Passive, // calls bind() listen() accept()
}

#[derive(Debug, Clone)]
pub(crate) enum NonsyncBlocker {
    Inconsistent,
    ComponentExit,
    SyncBlockStart,
}
#[derive(Debug, Clone)]
pub(crate) enum SyncBlocker {
    Inconsistent,
    SyncBlockEnd,
    CouldntReadMsg(PortId),
    CouldntCheckFiring(PortId),
    PutMsg(PortId, Payload),
    NondetChoice { n: u16 },
}
pub(crate) struct DenseDebugHex<'a>(pub &'a [u8]);

///////////////////// IMPL /////////////////////
impl IdParts for Id {
    fn id_parts(self) -> (ConnectorId, U32Suffix) {
        (self.connector_id, self.u32_suffix)
    }
}
impl IdParts for PortId {
    fn id_parts(self) -> (ConnectorId, U32Suffix) {
        self.0.id_parts()
    }
}
impl IdParts for ComponentId {
    fn id_parts(self) -> (ConnectorId, U32Suffix) {
        self.0.id_parts()
    }
}
impl U32Stream {
    pub(crate) fn next(&mut self) -> u32 {
        if self.next == u32::MAX {
            panic!("NO NEXT!")
        }
        self.next += 1;
        self.next - 1
    }
    pub(crate) fn n_skipped(mut self, n: u32) -> Self {
        self.next = self.next.saturating_add(n);
        self
    }
}
impl From<Id> for PortId {
    fn from(id: Id) -> PortId {
        Self(id)
    }
}
impl From<Id> for ComponentId {
    fn from(id: Id) -> Self {
        Self(id)
    }
}
impl From<&[u8]> for Payload {
    fn from(s: &[u8]) -> Payload {
        Payload(Arc::new(s.to_vec()))
    }
}
impl Payload {
    /// Create a new payload of uninitialized bytes with the given length.
    pub fn new(len: usize) -> Payload {
        let mut v = Vec::with_capacity(len);
        unsafe {
            v.set_len(len);
        }
        Payload(Arc::new(v))
    }
    /// Returns the length of the payload's byte sequence
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Allows shared reading of the payload's contents
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Allows mutation of the payload's contents.
    /// Results in a deep copy in the event this payload is aliased.
    pub fn as_mut_vec(&mut self) -> &mut Vec<u8> {
        Arc::make_mut(&mut self.0)
    }

    /// Modifies this payload, concatenating the given immutable payload's contents.
    /// Results in a deep copy in the event this payload is aliased.
    pub fn concatenate_with(&mut self, other: &Self) {
        let bytes = other.as_slice().iter().copied();
        let me = self.as_mut_vec();
        me.extend(bytes);
    }
}
impl serde::Serialize for Payload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let inner: &Vec<u8> = &self.0;
        inner.serialize(serializer)
    }
}
impl<'de> serde::Deserialize<'de> for Payload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let inner: Vec<u8> = Vec::deserialize(deserializer)?;
        Ok(Self(Arc::new(inner)))
    }
}
impl From<Vec<u8>> for Payload {
    fn from(s: Vec<u8>) -> Self {
        Self(s.into())
    }
}
impl Debug for PortId {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        let (a, b) = self.id_parts();
        write!(f, "pid{}_{}", a, b)
    }
}
impl Debug for ComponentId {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        let (a, b) = self.id_parts();
        write!(f, "cid{}_{}", a, b)
    }
}
impl Debug for Payload {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "Payload[{:?}]", DenseDebugHex(self.as_slice()))
    }
}
impl std::ops::Not for Polarity {
    type Output = Self;
    fn not(self) -> Self::Output {
        use Polarity::*;
        match self {
            Putter => Getter,
            Getter => Putter,
        }
    }
}
impl Debug for DenseDebugHex<'_> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        for b in self.0 {
            write!(f, "{:02X?}", b)?;
        }
        Ok(())
    }
}
