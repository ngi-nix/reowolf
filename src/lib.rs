#[macro_use]
mod macros;

mod common;
mod protocol;
mod runtime;

// #[cfg(test)]
// mod test;

pub use common::Polarity;
pub use protocol::ProtocolDescription;
pub use runtime::{Connector, EndpointSetup, StringLogger};

// #[cfg(feature = "ffi")]
// pub use runtime::ffi;
