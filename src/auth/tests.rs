//! Behavioural and redaction tests for the auth module.
//!
//! These tests intentionally live in a single module rather than per-file
//! because they cut across the three submodules (persistence, verification,
//! OAuth) and redaction invariants have to be checked against the public
//! surface — splitting them would duplicate fixtures.

use super::{
    ApiKeyVerifier, AuthStore, Credential, OAuthManager, PendingApiKey, VerificationStatus, cancellable_oauth,
    store_api_key_after_verification,
};
use crate::engine::provider::ProviderId;
use crate::error::AppError;
use std::path::PathBuf;
use std::sync::Mutex;

use super::oauth::map_oauth_error;

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rho_{name}_{}", uuid::Uuid::new_v4()))
}

#[test]
fn environment_key_precedes_stored_key() {
    let mut store = AuthStore::default();
    store.credentials.insert(
        "openai".to_string(),
        Credential::ApiKey {
            key: "stored-sentinel".to_string(),
        },
    );
    let key = store
        .get_key_with("openai", |name| {
            (name == "OPENAI_API_KEY").then(|| " environment-sentinel ".to_string())
        })
        .unwrap();
    assert_eq!(key.as_deref(), Some("environment-sentinel"));
}

#[test]
fn stored_key_is_used_when_environment_is_empty() {
    let mut store = AuthStore::default();
    store.credentials.insert(
        "anthropic".to_string(),
        Credential::ApiKey {
            key: "stored-value".to_string(),
        },
    );
    assert_eq!(
        store.get_key_with("anthropic", |_| None).unwrap().as_deref(),
        Some("stored-value")
    );
}

#[test]
fn oauth_entries_never_leave_api_key_interface() {
    let sentinel = "oauth-access-secret-sentinel";
    let mut store = AuthStore::default();
    store.credentials.insert(
        "openai".to_string(),
        Credential::OAuth {
            access_token: sentinel.to_string(),
            refresh_token: Some("refresh-secret-sentinel".to_string()),
            expires_at: None,
            endpoint: None,
        },
    );
    let error = store.get_key_with("openai", |_| None).unwrap_err();
    assert!(error.to_string().contains("Legacy OAuth credential"));
    assert!(!error.to_string().contains(sentinel));
    assert!(store.get_key_with("chatgpt", |_| None).is_err());
}

struct FakeVerifier {
    outcome: crate::error::Result<VerificationStatus>,
    calls: Mutex<Vec<ProviderId>>,
}

#[async_trait::async_trait]
impl ApiKeyVerifier for FakeVerifier {
    async fn verify(&self, provider: ProviderId, _key: &str) -> crate::error::Result<VerificationStatus> {
        self.calls.lock().unwrap().push(provider);
        match &self.outcome {
            Ok(status) => Ok(*status),
            Err(_) => Err(AppError::Auth("credential verification failed".to_string())),
        }
    }
}

#[tokio::test]
async fn verification_succeeds_before_key_is_replaced() {
    let path = temp_path("verified_auth.json");
    let mut store = AuthStore::load(&path).unwrap();
    let verifier = FakeVerifier {
        outcome: Ok(VerificationStatus::Verified),
        calls: Mutex::new(Vec::new()),
    };
    let pending = PendingApiKey {
        provider: ProviderId::OpenAi,
        key: "new-key".to_string(),
    };
    let status = store_api_key_after_verification(&mut store, pending, &verifier)
        .await
        .unwrap();
    assert_eq!(status, VerificationStatus::Verified);
    assert_eq!(
        store.get_key_with("openai", |_| None).unwrap().as_deref(),
        Some("new-key")
    );
    assert_eq!(*verifier.calls.lock().unwrap(), vec![ProviderId::OpenAi]);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn failed_verification_preserves_existing_key() {
    let path = temp_path("failed_auth.json");
    let mut store = AuthStore::load(&path).unwrap();
    store.set_api_key("openai", "existing-key".to_string()).unwrap();
    let verifier = FakeVerifier {
        outcome: Err(AppError::Auth("secret upstream body".to_string())),
        calls: Mutex::new(Vec::new()),
    };
    assert!(
        store_api_key_after_verification(
            &mut store,
            PendingApiKey {
                provider: ProviderId::OpenAi,
                key: "rejected-key".to_string(),
            },
            &verifier,
        )
        .await
        .is_err()
    );
    assert_eq!(
        store.get_key_with("openai", |_| None).unwrap().as_deref(),
        Some("existing-key")
    );
    let loaded = AuthStore::load(&path).unwrap();
    assert_eq!(
        loaded.get_key_with("openai", |_| None).unwrap().as_deref(),
        Some("existing-key")
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn deferred_verification_is_reported() {
    let path = temp_path("deferred_auth.json");
    let mut store = AuthStore::load(&path).unwrap();
    let verifier = FakeVerifier {
        outcome: Ok(VerificationStatus::Deferred),
        calls: Mutex::new(Vec::new()),
    };
    let pending = PendingApiKey {
        provider: ProviderId::Anthropic,
        key: "key".to_string(),
    };
    let status = store_api_key_after_verification(&mut store, pending, &verifier)
        .await
        .unwrap();
    assert_eq!(status, VerificationStatus::Deferred);
    let _ = std::fs::remove_file(path);
}

#[test]
fn provider_token_directories_are_distinct() {
    let root = temp_path("oauth_dirs");
    let manager = OAuthManager::new(&root);
    assert_eq!(
        manager.token_dir(ProviderId::ChatGpt).unwrap(),
        root.join("tokens/chatgpt")
    );
    assert_eq!(
        manager.token_dir(ProviderId::Copilot).unwrap(),
        root.join("tokens/copilot")
    );
    assert_ne!(
        manager.token_dir(ProviderId::ChatGpt).unwrap(),
        manager.token_dir(ProviderId::Copilot).unwrap()
    );
}

#[tokio::test]
async fn noninteractive_chatgpt_reload_does_not_start_device_flow() {
    let root = temp_path("chatgpt_reload");
    let manager = OAuthManager::new(&root);
    let error = manager.reload(ProviderId::ChatGpt).await.unwrap_err();
    assert!(error.to_string().contains("missing, stale, or revoked"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn noninteractive_copilot_reload_does_not_start_device_flow() {
    let root = temp_path("copilot_reload");
    let manager = OAuthManager::new(&root);
    let error = manager.reload(ProviderId::Copilot).await.unwrap_err();
    assert!(error.to_string().contains("missing, stale, or revoked"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn device_login_cancellation_is_local_and_redacted() {
    let authorize = std::future::pending::<crate::error::Result<()>>();
    let cancel = std::future::ready(Ok(()));
    let error = cancellable_oauth(ProviderId::ChatGpt, authorize, cancel)
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Cancelled(_)));
    assert_eq!(error.to_string(), "Operation cancelled: chatgpt login cancelled");
}

#[test]
fn oauth_error_mapping_is_redacted_and_specific() {
    let sentinel = "access-token-secret-sentinel";
    let cases = [
        ("device authorization was denied", "cancelled or denied"),
        ("response did not include a token", "no usable subscription entitlement"),
        ("token exchange failed", "token exchange failed"),
        ("invalid_grant", "stale, or revoked"),
    ];
    for (message, expected) in cases {
        let error = map_oauth_error(ProviderId::Copilot, &format!("{message} {sentinel}"));
        assert!(error.to_string().contains(expected));
        assert!(!error.to_string().contains(sentinel));
    }
}

#[test]
fn logout_removes_only_selected_provider_files() {
    let root = temp_path("logout");
    let manager = OAuthManager::new(&root);
    let chatgpt = manager.token_dir(ProviderId::ChatGpt).unwrap();
    let copilot = manager.token_dir(ProviderId::Copilot).unwrap();
    std::fs::create_dir_all(&chatgpt).unwrap();
    std::fs::create_dir_all(&copilot).unwrap();
    std::fs::write(chatgpt.join("auth.json"), "secret-input").unwrap();
    std::fs::write(copilot.join("access-token"), "secret-input").unwrap();
    manager.logout(ProviderId::ChatGpt).unwrap();
    assert!(!chatgpt.join("auth.json").exists());
    assert!(copilot.join("access-token").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn credential_files_and_token_directories_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let auth_path = temp_path("permissions.json");
    let mut store = AuthStore::load(&auth_path).unwrap();
    store.set_api_key("openai", "secret-input".to_string()).unwrap();
    assert_eq!(
        std::fs::metadata(&auth_path).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let root = temp_path("token_permissions");
    let manager = OAuthManager::new(&root);
    let token_dir = manager.prepare_token_dir(ProviderId::ChatGpt).unwrap();
    assert_eq!(
        std::fs::metadata(&token_dir).unwrap().permissions().mode() & 0o777,
        0o700
    );

    let _ = std::fs::remove_file(auth_path);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn credential_debug_output_is_redacted() {
    let sentinel = "api-key-secret-sentinel";
    let formatted = format!(
        "{:?}",
        Credential::ApiKey {
            key: sentinel.to_string()
        }
    );
    assert!(!formatted.contains(sentinel));
    assert!(formatted.contains("REDACTED"));
}
