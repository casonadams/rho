//! Provider selection, model construction, credential verification, and model
//! catalog discovery.
//!
//! The module is organized by responsibility:
//!
//! - [`id`]: the [`ProviderId`] and [`CredentialStrategy`] enums, parsing, and
//!   per-provider metadata (env var name, auth-mode label).
//! - [`factory`]: the [`ProviderFactory`] used to build a rig [`ModelHandle`]
//!   for any provider, including the per-provider OAuth / Ollama builders.
//! - [`verifier`]: the [`RigCredentialVerifier`] used to confirm an API key
//!   against the upstream provider before storing it.
//! - [`catalog`]: live and curated model listings per provider via
//!   [`ModelCatalog`] and [`list_models`].
//!
//! The public surface (`ProviderId`, `CredentialStrategy`, `ProviderFactory`,
//! `ModelRequest`, `RigCredentialVerifier`, `ModelCatalog`, `list_models`,
//! `curated`) is preserved exactly.

mod catalog;
mod factory;
pub mod host_loop;
pub mod registry;
mod verifier;

pub use catalog::{ModelCatalog, curated, list_models};
pub use factory::{ModelRequest, ProviderFactory};
pub use rho_core::provider::{CredentialStrategy, ProviderId};
pub use verifier::RigCredentialVerifier;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
