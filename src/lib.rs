#[macro_use]
mod macros;

mod common;
mod protocol;
mod runtime;

// #[cfg(test)]
// mod test;

pub use common::{ConnectorId, EndpointPolarity, Polarity, PortId};
pub use protocol::ProtocolDescription;
pub use runtime::{error, Connector, DummyLogger, FileLogger, VecLogger};

// #[cfg(feature = "ffi")]
// pub use runtime::ffi;
