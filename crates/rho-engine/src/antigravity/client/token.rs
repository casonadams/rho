//! Dynamic token resolution and refresh provider for Antigravity API clients.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::auth::store::AuthStore;

#[cfg(test)]
mod tests;

/// Provider abstraction for dynamic access token acquisition and refresh.
#[async_trait]
pub trait TokenProvider: Send + Sync {
    /// Retrieve current access token, proactively refreshing if expired.
    async fn token(&self) -> Result<String, String>;

    /// Force a refresh of the access token regardless of expiry time.
    async fn force_refresh(&self) -> Result<String, String>;
}

/// A fixed token provider that never changes or refreshes (used for testing or static keys).
#[derive(Debug, Clone)]
pub struct StaticTokenProvider(String);

impl StaticTokenProvider {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }
}

#[async_trait]
impl TokenProvider for StaticTokenProvider {
    async fn token(&self) -> Result<String, String> {
        Ok(self.0.clone())
    }

    async fn force_refresh(&self) -> Result<String, String> {
        Ok(self.0.clone())
    }
}

/// Token provider backed by an `AuthStore` that automatically refreshes OAuth credentials.
#[derive(Clone)]
pub struct AuthStoreTokenProvider {
    store: Arc<Mutex<AuthStore>>,
    provider: String,
}

impl AuthStoreTokenProvider {
    pub fn new(store: Arc<Mutex<AuthStore>>, provider: impl Into<String>) -> Self {
        Self {
            store,
            provider: provider.into(),
        }
    }
}

#[async_trait]
impl TokenProvider for AuthStoreTokenProvider {
    async fn token(&self) -> Result<String, String> {
        let mut guard = self.store.lock().await;
        guard
            .get_key(&self.provider)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("No credential found for provider '{}'", self.provider))
    }

    async fn force_refresh(&self) -> Result<String, String> {
        let mut guard = self.store.lock().await;
        guard
            .force_refresh(&self.provider)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Failed to refresh credential for provider '{}'", self.provider))
    }
}
