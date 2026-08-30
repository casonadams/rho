//! Built-in provider identity, catalogs, oauth, and scoped-credential operations.

pub mod auth;
pub mod catalog;
pub mod factory;
pub mod quota;
pub mod registry;
pub mod verifier;

pub use crate::catalog::{ModelCatalog, curated, list_models};
pub use crate::factory::{ModelRequest, ProviderFactory};
pub use crate::quota::{
    QuotaWindow, fetch_chatgpt_quota, fetch_ollama_cloud_quota, format_quota_windows, is_ollama_cloud_model,
};
pub use crate::registry::{ActiveProvider, BuiltinProvider, ProviderFacts, ProviderRegistry, context_limit};
pub use crate::verifier::RigCredentialVerifier;

pub use rho_core::provider::{CredentialStrategy, ProviderId};
