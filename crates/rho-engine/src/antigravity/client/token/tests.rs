use super::*;
use rho_harness_core::auth::StoredCredential;
use tempfile::NamedTempFile;

#[tokio::test]
async fn static_provider_returns_configured_token() {
    let provider = StaticTokenProvider::new("test-token-123");
    assert_eq!(provider.token().await.unwrap(), "test-token-123");
    assert_eq!(provider.force_refresh().await.unwrap(), "test-token-123");
}

#[tokio::test]
async fn auth_store_provider_resolves_valid_token() {
    let file = NamedTempFile::new().unwrap();
    let mut store = AuthStore::load(file.path()).unwrap();
    store
        .set_credential(
            "antigravity",
            StoredCredential::oauth("active_access_token", Some("ref_token".into()), Some(1799999999000)),
        )
        .unwrap();

    let provider = AuthStoreTokenProvider::new(Arc::new(Mutex::new(store)), "antigravity");
    assert_eq!(provider.token().await.unwrap(), "active_access_token");
}

#[tokio::test]
async fn auth_store_provider_fails_for_unknown_provider() {
    let file = NamedTempFile::new().unwrap();
    let store = AuthStore::load(file.path()).unwrap();
    let provider = AuthStoreTokenProvider::new(Arc::new(Mutex::new(store)), "nonexistent");
    assert!(provider.token().await.is_err());
}
