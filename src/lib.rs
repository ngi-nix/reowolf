#[macro_use]
mod macros;

mod common;
mod protocol;
mod runtime;

pub use common::{ConnectorId, EndpointPolarity, Payload, Polarity, PortId};
pub use protocol::{ProtocolDescription, TRIVIAL_PD};
pub use runtime::{error, Connector, DummyLogger, FileLogger, VecLogger};

// TODO: Remove when not benchmarking
pub use protocol::inputsource::InputSource;
pub use protocol::ast::Heap;
pub use protocol::lexer::Lexer;

#[cfg(feature = "ffi")]
pub mod ffi;
