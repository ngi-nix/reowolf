#[macro_use]
mod macros;

mod common;
mod protocol;
mod runtime;

pub use common::{ConnectorId, EndpointPolarity, Payload, Polarity, PortId};
pub use protocol::ProtocolDescription;
pub use runtime::{error, Connector, DummyLogger, FileLogger, VecLogger};

#[cfg(feature = "ffi")]
pub mod ffi;
