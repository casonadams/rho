//! Plugin runtime: `daemon` supervises out-of-process plugins over the rho
//! JSON-RPC protocol, `host` serves host-UI calls from in-process plugins,
//! and `protocol` defines the shared wire types.

pub mod daemon;
pub mod host;
pub mod protocol;
pub mod trait_api;

pub use trait_api::*;
