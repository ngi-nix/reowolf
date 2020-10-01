use crate::common::*;

#[derive(Debug)]
pub enum ConnectError {
    BindFailed(SocketAddr),
    UdpConnectFailed(SocketAddr),
    TcpInvalidConnect(SocketAddr),
    PollInitFailed,
    Timeout,
    PollFailed,
    AcceptFailed(SocketAddr),
    AlreadyConnected,
    PortPeerPolarityMismatch(PortId),
    NetEndpointSetupError(SocketAddr, NetEndpointError),
    SetupAlgMisbehavior,
}
#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum AddComponentError {
    DuplicatePort(PortId),
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
    BrokenNetEndpoint { index: usize },
    BrokenUdpEndpoint { index: usize },
    MalformedStateError(MalformedStateError),
}
#[derive(Debug, Clone)]
pub enum SyncError {
    NotConnected,
    InconsistentProtoComponent(ComponentId),
    RoundFailure,
    Unrecoverable(UnrecoverableSyncError),
}
#[derive(Debug, Clone)]
pub enum MalformedStateError {
    PortCannotPut(PortId),
    GetterUnknownFor { putter: PortId },
}
#[derive(Debug, Clone)]
pub enum NetEndpointError {
    MalformedMessage,
    BrokenNetEndpoint,
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
pub struct WrongStateError;
/////////////////////
impl From<UnrecoverableSyncError> for SyncError {
    fn from(e: UnrecoverableSyncError) -> Self {
        Self::Unrecoverable(e)
    }
}
