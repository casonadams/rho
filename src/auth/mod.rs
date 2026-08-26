//! Authentication and credential storage for rust-ai.
//!
//! This module is organised by responsibility:
//!
//! - [`credential`]: the [`Credential`] data shape, the [`AuthStore`] persistence layer,
//!   and the on-disk private file permissions helper.
//! - [`verification`]: the [`ApiKeyVerifier`] trait, [`PendingApiKey`],
//!   [`VerificationStatus`], [`cancellable_oauth`], and
//!   [`store_api_key_after_verification`] — i.e. everything related to validating a
//!   key before it lands in the store.
//! - [`oauth`]: the [`OAuthManager`] for subscription providers plus the rig OAuth
//!   client builders ([`chatgpt_client`], [`copilot_client`]) and the OAuth-error
//!   normaliser that converts upstream messages into safe, redacted user-facing
//!   errors.
//!
//! The tests in [`tests`] exercise the whole module and live in `auth/tests.rs`.

mod credential;
mod oauth;
mod verification;

pub use credential::{AuthStore, Credential};
pub use oauth::{OAuthManager, chatgpt_client, copilot_client};
pub use verification::{
    ApiKeyVerifier, PendingApiKey, VerificationStatus, cancellable_oauth, store_api_key_after_verification,
};

#[cfg(test)]
mod tests;
