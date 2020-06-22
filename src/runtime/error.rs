use crate::common::*;

pub enum EndpointError {
    MalformedMessage,
    BrokenEndpoint,
}
pub enum TryRecyAnyError {
    Timeout,
    PollFailed,
    EndpointError { error: EndpointError, index: usize },
    BrokenEndpoint(usize),
}
pub enum SyncError {
    Timeout,
}
