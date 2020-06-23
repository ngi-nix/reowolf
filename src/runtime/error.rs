use crate::common::*;

#[derive(Debug)]
pub enum EndpointError {
    MalformedMessage,
    BrokenEndpoint,
}
#[derive(Debug)]
pub enum TryRecyAnyError {
    Timeout,
    PollFailed,
    EndpointError { error: EndpointError, index: usize },
    BrokenEndpoint(usize),
}
#[derive(Debug, Clone)]
pub enum SyncError {
    Timeout,
    NotConnected,
    InconsistentProtoComponent(ProtoComponentId),
    IndistinguishableBatches([usize; 2]),
    DistributedTimeout,
}
#[derive(Debug)]
pub enum PortOpError {
    WrongPolarity,
    NotConnected,
    MultipleOpsOnPort,
    PortUnavailable,
}
#[derive(Debug, Eq, PartialEq)]
pub enum GottenError {
    NoPreviousRound,
    PortDidntGet,
    PreviousSyncFailed,
}

#[derive(Debug, Eq, PartialEq)]
pub enum NextBatchError {
    NotConnected,
}
