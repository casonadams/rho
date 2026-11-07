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

fn store_with(provider: &str, credential: StoredCredential) -> (tempfile::TempDir, AuthStore) {
    let (dir, mut store) = crate::test_support::temp_auth_store();
    store.set_credential(provider, credential).unwrap();
    (dir, store)
}

#[tokio::test]
async fn env_references_in_api_keys_are_expanded() {
    unsafe { std::env::set_var("RHO_ENV_TEST_KEY", "env_secret_123") };
    let (_file, store) = store_with("custom", StoredCredential::api_key("$RHO_ENV_TEST_KEY".to_string()));

    assert_eq!(store.get_key_sync("custom").unwrap().as_deref(), Some("env_secret_123"));
    assert_eq!(store.resolved_secret_values().await, vec!["env_secret_123".to_string()]);
}

#[tokio::test]
async fn force_refresh_returns_api_key_directly() {
    let (_file, mut store) = store_with("anthropic", StoredCredential::api_key("sk-ant-api03-test".to_string()));

    let refreshed = store.force_refresh("anthropic").await.unwrap();
    assert_eq!(refreshed.as_deref(), Some("sk-ant-api03-test"));
}

#[tokio::test]
async fn force_refresh_missing_refresh_token_fails() {
    let (_file, mut store) = store_with(
        "antigravity",
        StoredCredential::oauth("stale_token", None, Some(1799999999000)),
    );

    let err = store.force_refresh("antigravity").await.unwrap_err();
    assert!(err.to_string().contains("No refresh token") || err.to_string().contains("expired"));
}

#[tokio::test]
async fn expired_oauth_token_that_cannot_refresh_is_an_error() {
    let (_file, mut store) = store_with("antigravity", StoredCredential::oauth("stale_token", None, Some(1_000)));

    assert!(store.get_key("antigravity").await.is_err());
}

#[test]
fn loads_auth_file_with_utf8_bom() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path();
    let bom_content = "\u{FEFF}{\"claude\": \"token-claude-123\", \"openrouter\": \"key-openrouter-456\"}";
    std::fs::write(path, bom_content).unwrap();

    let mut store = AuthStore::load(path).unwrap();
    assert_eq!(
        store.get_key_sync("claude").unwrap().as_deref(),
        Some("token-claude-123")
    );
    assert_eq!(
        store.get_key_sync("openrouter").unwrap().as_deref(),
        Some("key-openrouter-456")
    );

    // Removing openrouter should preserve claude, not truncate to empty
    store.remove_key("openrouter").unwrap();

    let reloaded = AuthStore::load(path).unwrap();
    assert_eq!(
        reloaded.get_key_sync("claude").unwrap().as_deref(),
        Some("token-claude-123")
    );
    assert_eq!(reloaded.get_key_sync("openrouter").unwrap(), None);
}

#[test]
fn unparseable_non_empty_auth_file_fails_closed() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path();
    std::fs::write(path, "{ corrupt json ...").unwrap();

    let result = AuthStore::load(path);
    assert!(result.is_err(), "Loading corrupt auth file must fail closed");
}

#[tokio::test]
async fn loads_auth_file_with_utf8_bom_async() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path();
    let bom_content = "\u{FEFF}{\"claude\": \"token-claude-123\", \"openrouter\": \"key-openrouter-456\"}";
    std::fs::write(path, bom_content).unwrap();

    let mut store = AuthStore::load_async(path).await.unwrap();
    assert_eq!(
        store.get_key("claude").await.unwrap().as_deref(),
        Some("token-claude-123")
    );
    assert_eq!(
        store.get_key("openrouter").await.unwrap().as_deref(),
        Some("key-openrouter-456")
    );

    store.remove_key_async("openrouter").await.unwrap();

    let mut reloaded = AuthStore::load_async(path).await.unwrap();
    assert_eq!(
        reloaded.get_key("claude").await.unwrap().as_deref(),
        Some("token-claude-123")
    );
    assert_eq!(reloaded.get_key("openrouter").await.unwrap(), None);
}

#[tokio::test]
async fn unparseable_non_empty_auth_file_fails_closed_async() {
    let file = NamedTempFile::new().unwrap();
    let path = file.path();
    std::fs::write(path, "{ corrupt json ...").unwrap();

    let result = AuthStore::load_async(path).await;
    assert!(result.is_err(), "Loading corrupt auth file must fail closed");
}
