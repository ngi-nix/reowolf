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
#[derive(Debug)]
pub enum SyncError {
    Timeout,
    NotConnected,
    InconsistentProtoComponent(ProtoComponentId),
    IndistinguishableBatches([usize; 2]),
}
#[derive(Debug)]
pub enum PortOpError {
    WrongPolarity,
    NotConnected,
    MultipleOpsOnPort,
    PortUnavailable,
}
