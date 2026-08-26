use crate::engine::provider::{CredentialStrategy, ProviderId};
use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Credential {
    #[serde(rename = "api_key")]
    ApiKey { key: String },
    #[serde(rename = "oauth")]
    OAuth {
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<i64>,
        endpoint: Option<String>,
    },
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey { .. } => f.write_str("Credential::ApiKey([REDACTED])"),
            Self::OAuth { .. } => f.write_str("Credential::OAuth([REDACTED])"),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthStore {
    pub credentials: HashMap<String, Credential>,
    #[serde(skip)]
    path: PathBuf,
}

impl AuthStore {
    pub fn load(path: &Path) -> Result<Self> {
        let mut store = if path.exists() {
            let data = std::fs::read_to_string(path)
                .map_err(|error| AppError::Auth(format!("Failed to read {}: {error}", path.display())))?;
            serde_json::from_str::<Self>(&data)
                .map_err(|_| AppError::Auth(format!("Credential store {} is malformed", path.display())))?
        } else {
            Self::default()
        };
        store.path = path.to_path_buf();
        Ok(store)
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_vec_pretty(self)
            .map_err(|error| AppError::Auth(format!("Failed to serialize auth store: {error}")))?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.path)?;
        file.write_all(&data)?;
        file.sync_all()?;
        set_private_file_permissions(&self.path)?;
        Ok(())
    }

    pub fn set_api_key(&mut self, provider: &str, key: String) -> Result<()> {
        let provider = ProviderId::from_str(provider)?;
        if provider.credential_strategy() != CredentialStrategy::ApiKey {
            return Err(AppError::Auth(format!(
                "{provider} does not accept API keys in the rust-ai credential store"
            )));
        }
        self.credentials
            .insert(provider.as_str().to_string(), Credential::ApiKey { key });
        self.save()
    }

    pub fn remove_provider_entry(&mut self, provider: &str) -> Result<()> {
        let provider = ProviderId::from_str(provider)?;
        self.credentials.remove(provider.as_str());
        self.save()
    }

    pub fn get_key(&self, provider: &str) -> Result<Option<String>> {
        self.get_key_with(provider, |name| std::env::var(name).ok())
    }

    pub(crate) fn secret_values(&self) -> Vec<String> {
        self.credentials
            .values()
            .flat_map(|credential| match credential {
                Credential::ApiKey { key } => vec![key.clone()],
                Credential::OAuth {
                    access_token,
                    refresh_token,
                    ..
                } => {
                    let mut values = vec![access_token.clone()];
                    values.extend(refresh_token.clone());
                    values
                }
            })
            .filter(|value| !value.is_empty())
            .collect()
    }

    fn get_key_with<F>(&self, provider: &str, get_env: F) -> Result<Option<String>>
    where
        F: Fn(&str) -> Option<String>,
    {
        let provider = ProviderId::from_str(provider)?;
        if provider.credential_strategy() != CredentialStrategy::ApiKey {
            return Err(AppError::Auth(format!(
                "{provider} does not expose OAuth credentials through the API-key interface"
            )));
        }

        if let Some(value) = provider.api_key_env().and_then(&get_env).and_then(non_empty) {
            return Ok(Some(value));
        }

        let generic_name = format!("{}_API_KEY", provider.as_str().to_ascii_uppercase());
        if let Some(value) = get_env(&generic_name).and_then(non_empty) {
            return Ok(Some(value));
        }

        match self.credentials.get(provider.as_str()) {
            Some(Credential::ApiKey { key }) => Ok(non_empty(key.clone())),
            Some(Credential::OAuth { .. }) => Err(AppError::Auth(format!(
                "Legacy OAuth credential found for {provider}; remove it and use subscription login"
            ))),
            None => Ok(None),
        }
    }
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

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

#[derive(Debug, Clone)]
pub struct OAuthManager {
    token_root: PathBuf,
}

impl OAuthManager {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            token_root: config_dir.join("tokens"),
        }
    }

    pub fn token_dir(&self, provider: ProviderId) -> Result<PathBuf> {
        if provider.credential_strategy() != CredentialStrategy::SubscriptionOAuth {
            return Err(AppError::Auth(format!("{provider} is not a subscription provider")));
        }
        Ok(self.token_root.join(provider.as_str()))
    }

    pub async fn login(&self, provider: ProviderId) -> Result<()> {
        let token_dir = self.prepare_token_dir(provider)?;
        let result = match provider {
            ProviderId::ChatGpt => chatgpt_client(&token_dir, true)?.authorize().await,
            ProviderId::Copilot => copilot_client(&token_dir, true)?.authorize().await,
            _ => {
                return Err(AppError::Auth(format!(
                    "{provider} does not support subscription login"
                )));
            }
        };
        result.map_err(|error| map_oauth_error(provider, &error.to_string()))?;
        secure_token_files(provider, &token_dir)?;
        Ok(())
    }

    pub async fn reload(&self, provider: ProviderId) -> Result<()> {
        let token_dir = self.prepare_token_dir(provider)?;
        let result = match provider {
            ProviderId::ChatGpt => chatgpt_client(&token_dir, false)?.authorize().await,
            ProviderId::Copilot => copilot_client(&token_dir, false)?.authorize().await,
            _ => {
                return Err(AppError::Auth(format!(
                    "{provider} does not support subscription login"
                )));
            }
        };
        result.map_err(|error| map_oauth_error(provider, &error.to_string()))?;
        secure_token_files(provider, &token_dir)?;
        Ok(())
    }

    pub async fn refresh_if_needed(&self, provider: ProviderId) -> Result<()> {
        self.reload(provider).await
    }

    pub fn logout(&self, provider: ProviderId) -> Result<()> {
        let token_dir = self.token_dir(provider)?;
        for file in token_files(provider, &token_dir) {
            match std::fs::remove_file(file) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        match std::fs::remove_dir(&token_dir) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    fn prepare_token_dir(&self, provider: ProviderId) -> Result<PathBuf> {
        let token_dir = self.token_dir(provider)?;
        std::fs::create_dir_all(&token_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&token_dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(token_dir)
    }
}

pub fn chatgpt_client(token_dir: &Path, interactive: bool) -> Result<rig::providers::chatgpt::Client> {
    rig::providers::chatgpt::Client::builder()
        .oauth()
        .token_dir(token_dir)
        .allow_device_flow(interactive)
        .on_device_code(|prompt| {
            println!("Open {} and enter code {}", prompt.verification_uri, prompt.user_code);
        })
        .build()
        .map_err(|_| AppError::Auth("Failed to initialize ChatGPT OAuth".to_string()))
}

pub fn copilot_client(token_dir: &Path, interactive: bool) -> Result<rig::providers::copilot::Client> {
    rig::providers::copilot::Client::builder()
        .oauth()
        .token_dir(token_dir)
        .allow_device_flow(interactive)
        .on_device_code(|prompt| {
            println!("Open {} and enter code {}", prompt.verification_uri, prompt.user_code);
        })
        .build()
        .map_err(|_| AppError::Auth("Failed to initialize Copilot OAuth".to_string()))
}

fn token_files(provider: ProviderId, token_dir: &Path) -> Vec<PathBuf> {
    match provider {
        ProviderId::ChatGpt => vec![token_dir.join("auth.json")],
        ProviderId::Copilot => vec![token_dir.join("access-token"), token_dir.join("api-key.json")],
        _ => Vec::new(),
    }
}

fn secure_token_files(provider: ProviderId, token_dir: &Path) -> Result<()> {
    for file in token_files(provider, token_dir) {
        if file.exists() {
            set_private_file_permissions(&file)?;
        }
    }
    Ok(())
}

fn map_oauth_error(provider: ProviderId, message: &str) -> AppError {
    let normalized = message.to_ascii_lowercase();
    let detail = if normalized.contains("denied") || normalized.contains("cancel") {
        "device authorization was cancelled or denied"
    } else if normalized.contains("did not include a token") || normalized.contains("entitlement") {
        "the account has no usable subscription entitlement"
    } else if normalized.contains("sign-in required")
        || normalized.contains("invalid_grant")
        || normalized.contains("401")
    {
        "stored credentials are missing, stale, or revoked; log in again"
    } else if normalized.contains("timed out") || normalized.contains("expired") {
        "device authorization expired or timed out"
    } else if normalized.contains("api key") || normalized.contains("token exchange") {
        "subscription token exchange failed"
    } else {
        "authentication failed"
    };
    AppError::Auth(format!("{provider} {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rust_ai_{name}_{}", uuid::Uuid::new_v4()))
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
        outcome: Result<VerificationStatus>,
        calls: Mutex<Vec<ProviderId>>,
    }

    #[async_trait::async_trait]
    impl ApiKeyVerifier for FakeVerifier {
        async fn verify(&self, provider: ProviderId, _key: &str) -> Result<VerificationStatus> {
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
        let authorize = std::future::pending::<Result<()>>();
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
}
