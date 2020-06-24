#[macro_use]
mod macros;

mod common;
mod protocol;
mod runtime;

// #[cfg(test)]
// mod test;

pub use common::{ControllerId, Polarity, PortId};
pub use protocol::ProtocolDescription;
pub use runtime::{error, Connector, EndpointSetup, FileLogger, VecLogger};

// #[cfg(feature = "ffi")]
// pub use runtime::ffi;
