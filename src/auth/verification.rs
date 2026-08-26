//! API-key verification flow.
//!
//! These types and functions are the seam between the [`AuthStore`](super::AuthStore)
//! persistence layer and the upstream providers. A [`PendingApiKey`] is the
//! in-flight candidate; [`cancellable_oauth`] races the device-flow authorization
//! against a user-cancellation signal; [`store_api_key_after_verification`]
//! verifies the key and only then writes it to disk.
//!
//! The trait [`ApiKeyVerifier`] lets tests substitute a deterministic verifier
//! in place of the real rig call so the success/failure/preserve-existing-key
//! paths can be exercised without network access.

use crate::auth::AuthStore;
use crate::engine::provider::ProviderId;
use crate::error::{AppError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationStatus {
    Verified,
    Deferred,
}

#[async_trait::async_trait]
pub trait ApiKeyVerifier: Send + Sync {
    async fn verify(&self, provider: ProviderId, key: &str) -> Result<VerificationStatus>;
}

pub struct PendingApiKey {
    pub provider: ProviderId,
    pub key: String,
}

pub async fn cancellable_oauth<F, C>(provider: ProviderId, authorize: F, cancel: C) -> Result<()>
where
    F: std::future::Future<Output = Result<()>>,
    C: std::future::Future<Output = std::io::Result<()>>,
{
    tokio::select! {
        result = authorize => result,
        signal = cancel => {
            signal?;
            Err(AppError::Cancelled(format!("{provider} login cancelled")))
        }
    }
}

pub async fn store_api_key_after_verification<V: ApiKeyVerifier>(
    store: &mut AuthStore,
    pending: PendingApiKey,
    verifier: &V,
) -> Result<VerificationStatus> {
    let status = verifier.verify(pending.provider, &pending.key).await?;
    store.set_api_key(pending.provider.as_str(), pending.key)?;
    Ok(status)
}
