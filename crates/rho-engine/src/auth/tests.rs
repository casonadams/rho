use super::*;
use rho_harness_core::auth::StoredCredential;
use tempfile::NamedTempFile;

#[test]
fn loads_legacy_flat_key_json() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path();
    std::fs::write(path, r#"{"anthropic": "sk-ant-legacy-123", "openai": "sk-legacy-456"}"#).unwrap();

    let store = AuthStore::load(path).unwrap();
    assert_eq!(
        store.get_key_sync("anthropic").unwrap().as_deref(),
        Some("sk-ant-legacy-123")
    );
    assert_eq!(store.get_key_sync("openai").unwrap().as_deref(), Some("sk-legacy-456"));
}

#[test]
fn stores_and_persists_oauth_credentials() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path();

    {
        let mut store = AuthStore::load(path).unwrap();
        let cred = StoredCredential::oauth("acc_999", Some("ref_888".into()), Some(1799999999000));
        store.set_credential("chatgpt", cred).unwrap();
    }

    let reloaded = AuthStore::load(path).unwrap();
    let cred = reloaded.get_credential("chatgpt").unwrap();
    assert_eq!(cred.raw_secret(), "acc_999");
    assert_eq!(reloaded.get_key_sync("chatgpt").unwrap().as_deref(), Some("acc_999"));
}

#[test]
fn expands_dynamic_key_resolvers() {
    unsafe { std::env::set_var("RHO_ENV_TEST_KEY", "env_secret_123") };

    let file = NamedTempFile::new().unwrap();
    let path = file.path();

    let mut store = AuthStore::load(path).unwrap();
    store.set_key("custom", "$RHO_ENV_TEST_KEY").unwrap();

    assert_eq!(store.get_key_sync("custom").unwrap().as_deref(), Some("env_secret_123"));
}

#[tokio::test]
async fn force_refresh_returns_api_key_directly() {
    let file = NamedTempFile::new().unwrap();
    let mut store = AuthStore::load(file.path()).unwrap();
    store.set_key("anthropic", "sk-ant-api03-test").unwrap();

    let refreshed = store.force_refresh("anthropic").await.unwrap();
    assert_eq!(refreshed.as_deref(), Some("sk-ant-api03-test"));
}

#[tokio::test]
async fn force_refresh_missing_refresh_token_fails() {
    let file = NamedTempFile::new().unwrap();
    let mut store = AuthStore::load(file.path()).unwrap();
    let cred = StoredCredential::oauth("stale_token", None, Some(1799999999000));
    store.set_credential("antigravity", cred).unwrap();

    let err = store.force_refresh("antigravity").await.unwrap_err();
    assert!(err.to_string().contains("No refresh token") || err.to_string().contains("expired"));
}
