use super::*;
use rho_harness_core::auth::StoredCredential;
use rho_test_support::TestFile;
use std::collections::HashMap;

#[test]
fn loads_legacy_flat_key_json() {
    let file = TestFile::new();
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
    let file = TestFile::new();
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

fn login_as(email: Option<&str>) -> StoredCredential {
    crate::test_support::oauth_login("tok", None, email)
}

#[test]
fn account_ids_come_from_the_login_then_the_token() {
    let (_dir, mut store) = crate::test_support::temp_auth_store();
    store
        .set_credential(
            "antigravity:me@work.com",
            crate::test_support::oauth_login("tok", Some("proj-abc"), Some("me@work.com")),
        )
        .unwrap();
    store.set_credential("antigravity", login_as(None)).unwrap();
    store.set_key("chatgpt", "sk-plain").unwrap();

    assert_eq!(store.account_id("antigravity:me@work.com").as_deref(), Some("proj-abc"));
    assert_eq!(store.account_id("antigravity"), None);
    assert_eq!(store.account_id("chatgpt"), None);
    assert_eq!(store.antigravity_project("antigravity:me@work.com"), "proj-abc");
    assert_eq!(
        store.antigravity_project("antigravity"),
        crate::auth::antigravity::default_project_id()
    );
    assert_eq!(store.chatgpt_account_id("chatgpt", "sk-plain"), None);
}

#[test]
fn new_logins_keep_existing_accounts_and_reuse_their_keys() {
    use rho_harness_core::provider::ProviderId::ClaudeCode;
    let key = |store: &AuthStore, email| store.login_key(ClaudeCode, &login_as(email)).to_string();
    let (_dir, mut store) = crate::test_support::temp_auth_store();
    assert_eq!(key(&store, Some("me@home.com")), "claude");
    store.set_credential("claude", login_as(Some("me@home.com"))).unwrap();

    let cases = [
        (Some("me@home.com"), "claude"),
        (Some("Me@Work.com"), "claude:me@work.com"),
        (None, "claude"),
    ];
    for (email, expected) in cases {
        assert_eq!(key(&store, email), expected, "{email:?}");
    }

    store
        .set_credential("claude:me@work.com", login_as(Some("me@work.com")))
        .unwrap();
    store.set_key("chatgpt", "k").unwrap();
    let logins: Vec<(String, Option<String>)> = store
        .logins(ClaudeCode)
        .into_iter()
        .map(|login| (login.connection.to_string(), login.email))
        .collect();
    assert_eq!(
        logins,
        vec![
            ("claude".to_string(), Some("me@home.com".to_string())),
            ("claude:me@work.com".to_string(), Some("me@work.com".to_string())),
        ]
    );
    assert_eq!(key(&store, Some("me@work.com")), "claude:me@work.com");
    assert_eq!(
        store
            .stored_logins()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["chatgpt", "claude", "claude:me@work.com"]
    );
}

#[tokio::test]
async fn env_references_in_api_keys_are_expanded() {
    unsafe { std::env::set_var("RHO_ENV_TEST_KEY", "env_secret_123") };
    let (_file, store) = store_with("custom", StoredCredential::api_key("$RHO_ENV_TEST_KEY".to_string()));

    assert_eq!(store.get_key_sync("custom").unwrap().as_deref(), Some("env_secret_123"));
    assert_eq!(store.resolved_secret_values().await, vec!["env_secret_123".to_string()]);
}

#[tokio::test]
async fn force_refresh_returns_api_keys_and_rejects_oauth_without_refresh_token() {
    let cases = [
        (
            "anthropic",
            StoredCredential::api_key("sk-ant-api03-test".to_string()),
            Some("sk-ant-api03-test"),
        ),
        (
            "antigravity",
            StoredCredential::oauth("stale_token", None, Some(1799999999000)),
            None,
        ),
        (
            "claude:me@work.com",
            StoredCredential::oauth("stale_token", None, Some(1_000)),
            None,
        ),
    ];
    for (provider, credential, expected_key) in cases {
        let (_file, mut store) = store_with(provider, credential);
        let result = AuthStore::force_refresh(&mut store, provider).await;
        match expected_key {
            Some(key) => assert_eq!(result.unwrap().as_deref(), Some(key)),
            None => {
                let message = result.unwrap_err().to_string();
                assert!(
                    message.contains("No refresh token") || message.contains("expired"),
                    "{provider}: {message}"
                );
            }
        }
    }
}

#[tokio::test]
async fn expired_oauth_token_that_cannot_refresh_is_an_error() {
    let (_file, mut store) = store_with("antigravity", StoredCredential::oauth("stale_token", None, Some(1_000)));

    assert!(store.get_key("antigravity").await.is_err());
}

#[test]
fn loads_auth_file_with_utf8_bom() {
    let file = TestFile::new();
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
    let file = TestFile::new();
    let path = file.path();
    std::fs::write(path, "{ corrupt json ...").unwrap();

    let result = AuthStore::load(path);
    assert!(result.is_err(), "Loading corrupt auth file must fail closed");
}

#[tokio::test]
async fn loads_auth_file_with_utf8_bom_async() {
    let file = TestFile::new();
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

    store.remove_key("openrouter").unwrap();

    let mut reloaded = AuthStore::load_async(path).await.unwrap();
    assert_eq!(
        reloaded.get_key("claude").await.unwrap().as_deref(),
        Some("token-claude-123")
    );
    assert_eq!(reloaded.get_key("openrouter").await.unwrap(), None);
}

#[tokio::test]
async fn unparseable_non_empty_auth_file_fails_closed_async() {
    let file = TestFile::new();
    let path = file.path();
    std::fs::write(path, "{ corrupt json ...").unwrap();

    let result = AuthStore::load_async(path).await;
    assert!(result.is_err(), "Loading corrupt auth file must fail closed");
}

#[tokio::test]
async fn refresh_adopts_credential_another_process_saved_to_disk() {
    let far_future = 4_102_444_800_000;
    let cases = [
        ("expired token", Some(1_000), false),
        ("rejected live token", Some(far_future), true),
    ];
    for (name, memory_expiry, force) in cases {
        let (dir, mut store) = store_with(
            "claude",
            StoredCredential::oauth("stale_access", Some("dead_refresh".into()), memory_expiry),
        );
        let mut other_process = AuthStore::load(dir.path().join("auth.json")).unwrap();
        let rotated = StoredCredential::oauth("fresh_access", Some("rotated_refresh".into()), Some(far_future));
        other_process.set_credential("claude", rotated.clone()).unwrap();

        let key = if force {
            store.force_refresh("claude").await
        } else {
            store.get_key("claude").await
        };

        assert_eq!(key.unwrap().as_deref(), Some("fresh_access"), "{name}");
        assert_eq!(store.get_credential("claude"), Some(&rotated), "{name}");
    }
}

#[test]
fn saving_from_a_stale_store_keeps_other_providers_rotated_credentials() {
    let (dir, mut stale) = store_with(
        "claude",
        StoredCredential::oauth("old_access", Some("old_refresh".into()), None),
    );
    let mut renewed = AuthStore::load(dir.path().join("auth.json")).unwrap();
    let rotated = StoredCredential::oauth("new_access", Some("new_refresh".into()), Some(4_102_444_800_000));
    renewed.set_credential("claude", rotated.clone()).unwrap();

    stale.set_key("openrouter", "sk-or-key").unwrap();
    stale.remove_key("openrouter").unwrap();

    let on_disk = AuthStore::load(dir.path().join("auth.json")).unwrap();
    assert_eq!(on_disk.get_credential("claude"), Some(&rotated));
}

#[tokio::test]
async fn refresh_never_replaces_memory_with_an_older_disk_credential() {
    let far_future = 4_102_444_800_000;
    let newer = StoredCredential::oauth("newer_access", Some("newer_refresh".into()), Some(far_future + 1_000));
    let (dir, mut store) = store_with("openrouter", newer.clone());
    let older = StoredCredential::oauth("older_access", Some("older_refresh".into()), Some(far_future));
    let mut stale_process = AuthStore::load(dir.path().join("auth.json")).unwrap();
    stale_process.set_credential("openrouter", older).unwrap();

    let key = store.force_refresh("openrouter").await.unwrap();

    assert_eq!(key.as_deref(), Some("newer_access"));
    let on_disk = AuthStore::load(dir.path().join("auth.json")).unwrap();
    assert_eq!(on_disk.get_credential("openrouter"), Some(&newer));
}

#[tokio::test]
async fn refresh_waits_for_another_process_holding_the_auth_lock() {
    let far_future = 4_102_444_800_000;
    let expired = StoredCredential::oauth("stale_access", Some("stale_refresh".into()), Some(1_000));
    let (dir, mut store) = store_with("openrouter", expired);
    let path = dir.path().join("auth.json");
    let other_process = std::fs::File::create(dir.path().join("auth.json.lock")).unwrap();
    other_process.lock().unwrap();

    let refresh = tokio::spawn(async move { store.get_key("openrouter").await });
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert!(!refresh.is_finished());
    let fresh = StoredCredential::oauth("fresh_access", Some("fresh_refresh".into()), Some(far_future));
    let disk = serde_json::to_string(&HashMap::from([("openrouter", &fresh)])).unwrap();
    std::fs::write(&path, disk).unwrap();
    other_process.unlock().unwrap();

    assert_eq!(refresh.await.unwrap().unwrap().as_deref(), Some("fresh_access"));
}
