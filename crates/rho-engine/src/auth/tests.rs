//! Behavioural and redaction tests for the auth module.
//!
//! These tests intentionally live in a single module rather than per-file
//! because they cut across the three submodules (persistence, verification,
//! OAuth) and redaction invariants have to be checked against the public
//! surface — splitting them would duplicate fixtures.

use super::{
    ApiKeyVerifier, AuthStore, Credential, CredentialScope, CredentialUpdate, OAuthManager, PendingApiKey,
    VerificationStatus, cancellable_oauth, store_api_key_after_verification,
};
use crate::engine::provider::registry::ProviderRegistry;

use crate::engine::provider::ProviderId;
use rho_core::error::AppError;
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
    outcome: rho_core::error::Result<VerificationStatus>,
    calls: Mutex<Vec<ProviderId>>,
}

#[async_trait::async_trait]
impl ApiKeyVerifier for FakeVerifier {
    async fn verify(&self, provider: ProviderId, _key: &str) -> rho_core::error::Result<VerificationStatus> {
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
    let authorize = std::future::pending::<rho_core::error::Result<()>>();
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

fn provider_scope() -> CredentialScope {
    CredentialScope::builtin_provider(ProviderId::Anthropic)
}

#[test]
fn scoped_credentials_round_trip_with_generation_cas() {
    let mut store = AuthStore::default();
    assert!(store.scoped_credential(&provider_scope()).is_none());

    let generation = store
        .compare_and_swap(CredentialUpdate {
            scope: provider_scope(),
            expected_generation: None,
            credential: Credential::ApiKey {
                key: "first-key".to_string(),
            },
        })
        .unwrap();
    assert_eq!(generation, 1);

    let (_, envelope) = store.scoped_credential(&provider_scope()).unwrap();
    assert_eq!(envelope.kind, "api_key.v1");
    assert_eq!(envelope.value["key"], "first-key");

    let replacement = store
        .compare_and_swap(CredentialUpdate {
            scope: provider_scope(),
            expected_generation: Some(generation),
            credential: Credential::ApiKey {
                key: "second-key".to_string(),
            },
        })
        .unwrap();
    assert_eq!(replacement, 2);
    let (_, envelope) = store.scoped_credential(&provider_scope()).unwrap();
    assert_eq!(envelope.value["key"], "second-key");
}

#[tokio::test]
async fn stale_scoped_refresh_is_rejected_without_overwriting_newer_material() {
    let mut store = AuthStore::default();
    let stale = store
        .compare_and_swap(CredentialUpdate {
            scope: provider_scope(),
            expected_generation: None,
            credential: Credential::ApiKey {
                key: "stale-key".to_string(),
            },
        })
        .unwrap();

    store
        .compare_and_swap(CredentialUpdate {
            scope: provider_scope(),
            expected_generation: Some(stale),
            credential: Credential::ApiKey {
                key: "fresh-key".to_string(),
            },
        })
        .unwrap();

    let error = store
        .compare_and_swap(CredentialUpdate {
            scope: provider_scope(),
            expected_generation: Some(stale),
            credential: Credential::ApiKey {
                key: "racing-key".to_string(),
            },
        })
        .unwrap_err();
    assert!(error.to_string().contains("stale"));
    let (_, envelope) = store.scoped_credential(&provider_scope()).unwrap();
    assert_eq!(envelope.value["key"], "fresh-key");
}

#[test]
fn scoped_credentials_survive_save_and_load_and_logout_isolation() {
    let path = temp_path("scoped_store.json");
    let mut store = AuthStore::load(&path).unwrap();
    store
        .compare_and_swap(CredentialUpdate {
            scope: provider_scope(),
            expected_generation: None,
            credential: Credential::ApiKey {
                key: "scoped-secret".to_string(),
            },
        })
        .unwrap();
    let copilot = CredentialScope::builtin_provider(ProviderId::Copilot);
    store
        .compare_and_swap(CredentialUpdate {
            scope: copilot.clone(),
            expected_generation: None,
            credential: Credential::ApiKey {
                key: "other-secret".to_string(),
            },
        })
        .unwrap();

    let loaded = AuthStore::load(&path).unwrap();
    let (_, envelope) = loaded.scoped_credential(&provider_scope()).unwrap();
    assert_eq!(envelope.value["key"], "scoped-secret");
    assert_eq!(loaded.scoped_credential(&copilot).unwrap().0, 1);

    let mut loaded = loaded;
    loaded.remove_scope(&provider_scope()).unwrap();
    assert!(loaded.scoped_credential(&provider_scope()).is_none());
    assert!(loaded.scoped_credential(&copilot).is_some());
    let _ = std::fs::remove_file(path);
}

#[test]
fn scoped_credential_debug_output_is_redacted() {
    let sentinel = "scoped-secret-sentinel";
    let mut store = AuthStore::default();
    store
        .compare_and_swap(CredentialUpdate {
            scope: provider_scope(),
            expected_generation: None,
            credential: Credential::ApiKey {
                key: sentinel.to_string(),
            },
        })
        .unwrap();
    let formatted = format!("{store:?}");
    assert!(!formatted.contains(sentinel));
}

#[tokio::test]
async fn provider_requests_carry_only_the_selected_scoped_credential() {
    use rho_sdk::contract::{ProviderRequest, ProviderToolDefinition};

    let mut store = AuthStore::default();
    store
        .compare_and_swap(CredentialUpdate {
            scope: CredentialScope::builtin_provider(ProviderId::OpenAi),
            expected_generation: None,
            credential: Credential::ApiKey {
                key: "openai-only-secret".to_string(),
            },
        })
        .unwrap();
    store
        .compare_and_swap(CredentialUpdate {
            scope: CredentialScope::builtin_provider(ProviderId::Anthropic),
            expected_generation: None,
            credential: Credential::ApiKey {
                key: "anthropic-secret".to_string(),
            },
        })
        .unwrap();

    let registry = ProviderRegistry::builtins();
    let selected = registry.selected_credential("openai", &store).unwrap().unwrap();
    assert_eq!(selected.1.value["key"], "openai-only-secret");
    let other = registry.selected_credential("anthropic", &store).unwrap().unwrap();
    assert_eq!(other.1.value["key"], "anthropic-secret");

    let request = ProviderRequest {
        model: "fixture".to_string(),
        messages: Vec::new(),
        credential: Some(selected.1),
        max_output_tokens: None,
        tools: vec![ProviderToolDefinition {
            id: "tool:read".parse().unwrap(),
            description: "read".to_string(),
            argument_schema: serde_json::json!({"type":"object"}),
        }],
    };
    let encoded = serde_json::to_string(&request).unwrap();
    assert!(encoded.contains("openai-only-secret"));
    assert!(!encoded.contains("anthropic-secret"));
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
