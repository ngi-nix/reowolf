use crate::common::*;

#[derive(Debug)]
pub enum ConnectError {
    BindFailed(SocketAddr),
    PollInitFailed,
    Timeout,
    PollFailed,
    AcceptFailed(SocketAddr),
    AlreadyConnected,
    PortPeerPolarityMismatch(PortId),
    EndpointSetupError(SocketAddr, EndpointError),
    SetupAlgMisbehavior,
}
////////////////////////
#[derive(Debug, Clone)]
pub enum SyncError {
    NotConnected,
    InconsistentProtoComponent(ProtoComponentId),
    IndistinguishableBatches([usize; 2]),
    RoundFailure,
    PollFailed,
    BrokenEndpoint(usize),
}
#[derive(Debug, Clone)]
pub enum EndpointError {
    MalformedMessage,
    BrokenEndpoint,
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
