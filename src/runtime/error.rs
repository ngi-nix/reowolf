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
////////////////////////
#[derive(Debug, Clone)]
pub enum UnrecoverableSyncError {
    PollFailed,
    BrokenEndpoint(usize),
    MalformedStateError(MalformedStateError),
}
#[derive(Debug, Clone)]
pub enum SyncError {
    NotConnected,
    InconsistentProtoComponent(ProtoComponentId),
    RoundFailure,
    Unrecoverable(UnrecoverableSyncError),
}
#[derive(Debug, Clone)]
pub enum MalformedStateError {
    PortCannotPut(PortId),
    GetterUnknownFor { putter: PortId },
}
#[derive(Debug, Clone)]
pub enum EndpointError {
    MalformedMessage,
    BrokenEndpoint,
}
#[derive(Debug)]
pub enum PortOpError {
    WrongPolarity,
    UnknownPolarity,
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

#[derive(Debug, Eq, PartialEq)]
pub enum NewNetPortError {
    AlreadyConnected,
}
/////////////////////
impl From<UnrecoverableSyncError> for SyncError {
    fn from(e: UnrecoverableSyncError) -> Self {
        Self::Unrecoverable(e)
    }
}
