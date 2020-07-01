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
// pub(crate) use indexmap::IndexSet;
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

pub type ConnectorId = u32;
pub type PortSuffix = u32;
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, Hash, PartialOrd, serde::Serialize, serde::Deserialize,
)]
// acquired via error in the Rust API
pub struct ProtoComponentId(Id);
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, Hash, PartialOrd, serde::Serialize, serde::Deserialize,
)]
#[repr(C)]
pub struct Id {
    pub(crate) connector_id: ConnectorId,
    pub(crate) u32_suffix: PortSuffix,
}
#[derive(Debug, Default)]
pub struct U32Stream {
    next: u32,
}
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, Hash, PartialOrd, serde::Serialize, serde::Deserialize,
)]
#[repr(transparent)]
pub struct PortId(Id);
#[derive(Default, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Payload(Arc<Vec<u8>>);
#[derive(
    Debug, Eq, PartialEq, Clone, Hash, Copy, Ord, PartialOrd, serde::Serialize, serde::Deserialize,
)]
#[repr(C)]
pub enum Polarity {
    Putter, // output port (from the perspective of the component)
    Getter, // input port (from the perspective of the component)
}
#[derive(
    Debug, Eq, PartialEq, Clone, Hash, Copy, Ord, PartialOrd, serde::Serialize, serde::Deserialize,
)]
#[repr(C)]
pub enum EndpointPolarity {
    Active,  // calls connect()
    Passive, // calls bind() listen() accept()
}
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, Hash, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct FiringVar(pub(crate) PortId);

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
}

///////////////////// IMPL /////////////////////
impl U32Stream {
    pub(crate) fn next(&mut self) -> u32 {
        if self.next == u32::MAX {
            panic!("NO NEXT!")
        }
        self.next += 1;
        self.next - 1
    }
}
impl From<Id> for PortId {
    fn from(id: Id) -> PortId {
        Self(id)
    }
}
impl From<Id> for ProtoComponentId {
    fn from(id: Id) -> ProtoComponentId {
        Self(id)
    }
}
impl From<&[u8]> for Payload {
    fn from(s: &[u8]) -> Payload {
        Payload(Arc::new(s.to_vec()))
    }
}
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
    pub fn concatenate_with(&mut self, other: &Self) {
        let bytes = other.as_slice().iter().copied();
        let me = Arc::make_mut(&mut self.0);
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
        write!(f, "ptID({}'{})", self.0.connector_id, self.0.u32_suffix)
    }
}
impl Debug for FiringVar {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "fvID({}'{})", (self.0).0.connector_id, (self.0).0.u32_suffix)
    }
}
impl Debug for ProtoComponentId {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "pcID({}'{})", self.0.connector_id, self.0.u32_suffix)
    }
}
impl Debug for Payload {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "Payload{:x?}", self.as_slice())
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
