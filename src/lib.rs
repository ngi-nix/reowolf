#[macro_use]
mod macros;

mod common;
mod protocol;
mod runtime;
mod collections;

pub use common::{ConnectorId, EndpointPolarity, Payload, Polarity, PortId};
pub use protocol::ProtocolDescription;
pub use runtime::{error, Connector, DummyLogger, FileLogger, VecLogger};

// TODO: Remove when not benchmarking
pub use protocol::input_source::InputSource;
pub use protocol::ast::Heap;

#[cfg(feature = "ffi")]
pub mod ffi;
