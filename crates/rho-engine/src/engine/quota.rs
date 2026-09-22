use std::str::FromStr;
use std::sync::Arc;

use rho_harness_core::auth::StoredCredential;
use rho_harness_core::provider::ProviderId;

use super::AgentEngine;
use crate::auth::AuthStore;
use crate::engine::tracking::{QuotaKey, QuotaTracker};

pub(crate) fn canonical_quota_provider(provider: &str) -> Option<&'static str> {
    let trimmed = provider.trim();
    if trimmed.eq_ignore_ascii_case("openai-chatgpt") {
        return Some("chatgpt");
    }
    match ProviderId::from_str(trimmed).ok()? {
        ProviderId::OllamaCloud => Some("ollama-cloud"),
        ProviderId::Antigravity => Some("antigravity"),
        ProviderId::ChatGpt => Some("chatgpt"),
        ProviderId::ClaudeCode => Some("claude"),
        ProviderId::Gemini => Some("gemini"),
        _ => None,
    }
}

impl AgentEngine {
    pub async fn refresh_quota(&self) {
        let Some(provider) = canonical_quota_provider(&self.config.provider) else {
            return;
        };
        match provider {
            "ollama-cloud" => {
                do_refresh_ollama_quota(Arc::clone(&self.auth_store), self.quota.clone()).await;
            }
            "antigravity" => {
                do_refresh_antigravity_quota(
                    Arc::clone(&self.auth_store),
                    self.quota.clone(),
                    self.config.model.clone(),
                )
                .await;
            }
            "chatgpt" => {
                do_refresh_chatgpt_quota(Arc::clone(&self.auth_store), self.quota.clone()).await;
            }
            "claude" => {
                do_refresh_claude_quota(
                    Arc::clone(&self.auth_store),
                    self.quota.clone(),
                    self.config.model.clone(),
                )
                .await;
            }
            "gemini" => {
                do_refresh_gemini_quota(
                    &self.config.sessions_dir,
                    Some(&self.session_manager.session_id),
                    self.session_usage_totals(),
                    &self.config.model,
                    self.quota.clone(),
                )
                .await;
            }
            _ => {}
        }
    }

    pub fn spawn_refresh_quota(&self) {
        let Some(provider) = canonical_quota_provider(&self.config.provider) else {
            return;
        };
        let auth = Arc::clone(&self.auth_store);
        let quota = self.quota.clone();
        match provider {
            "ollama-cloud" => {
                tokio::spawn(async move {
                    do_refresh_ollama_quota(auth, quota).await;
                });
            }
            "antigravity" => {
                let model = self.config.model.clone();
                tokio::spawn(async move {
                    do_refresh_antigravity_quota(auth, quota, model).await;
                });
            }
            "chatgpt" => {
                tokio::spawn(async move {
                    do_refresh_chatgpt_quota(auth, quota).await;
                });
            }
            "claude" => {
                let model = self.config.model.clone();
                tokio::spawn(async move {
                    do_refresh_claude_quota(auth, quota, model).await;
                });
            }
            "gemini" => {
                let sessions_dir = self.config.sessions_dir.clone();
                let sid = self.session_manager.session_id.clone();
                let totals = self.session_usage_totals();
                let model = self.config.model.clone();
                tokio::spawn(async move {
                    do_refresh_gemini_quota(&sessions_dir, Some(&sid), totals, &model, quota).await;
                });
            }
            _ => {}
        }
    }

    pub fn quota_display(&self) -> Option<String> {
        let provider = canonical_quota_provider(&self.config.provider)?;
        let key = QuotaKey::new(provider, Some(&self.config.model));
        self.quota.display_for(&key)
    }

    pub fn quota(&self) -> &QuotaTracker {
        &self.quota
    }

    pub fn should_refresh_quota(&self) -> bool {
        let Some(provider) = canonical_quota_provider(&self.config.provider) else {
            return false;
        };
        let key = QuotaKey::new(provider, Some(&self.config.model));
        self.quota.should_fetch(&key)
    }

    pub fn quota_subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.quota.subscribe()
    }

    pub fn quota_version(&self) -> u64 {
        self.quota.version()
    }

    pub fn invalidate_quota(&self) {
        let Some(provider) = canonical_quota_provider(&self.config.provider) else {
            return;
        };
        let key = QuotaKey::new(provider, Some(&self.config.model));
        self.quota.invalidate(&key);
        let fallback_key = QuotaKey::new(provider, None::<String>);
        self.quota.invalidate(&fallback_key);
    }

    pub fn force_refresh_quota(&self) {
        self.invalidate_quota();
        self.spawn_refresh_quota();
    }
}

fn record_quota_result(quota: &QuotaTracker, key: &QuotaKey, display: Option<String>) {
    match display {
        Some(display) => quota.record_success(key, display),
        None => quota.record_failure(key),
    }
}

fn stored_oauth_account_id(cred: Option<&StoredCredential>) -> Option<String> {
    match cred {
        Some(StoredCredential::OAuth {
            account_id: Some(id), ..
        }) => Some(id.clone()),
        _ => None,
    }
}

fn is_synthetic_project_id(id: &str) -> bool {
    let trimmed = id.trim();
    trimmed.is_empty() || uuid::Uuid::parse_str(trimmed).is_ok()
}

fn fallback_antigravity_project(current_id: String) -> String {
    if current_id.is_empty() {
        crate::auth::antigravity::stable_project_id("antigravity-default")
    } else {
        current_id
    }
}

fn update_antigravity_project(store: &mut AuthStore, discovered: &str) {
    if let Some(mut cred) = store.get_credential("antigravity").cloned() {
        if let StoredCredential::OAuth { ref mut account_id, .. } = cred {
            *account_id = Some(discovered.to_string());
        }
        let _ = store.set_credential("antigravity", cred);
    }
}

async fn resolve_antigravity_credentials(auth_store: &tokio::sync::Mutex<AuthStore>) -> Option<(String, String)> {
    let (token, current_id) = {
        let mut store = auth_store.lock().await;
        let token = store.get_key("antigravity").await.ok().flatten()?;
        let current_id = stored_oauth_account_id(store.get_credential("antigravity")).unwrap_or_default();
        (token, current_id)
    };

    if !is_synthetic_project_id(&current_id) {
        return Some((token, current_id));
    }

    if let Some(discovered) = crate::antigravity::client::load_project_id(&token).await
        && !is_synthetic_project_id(&discovered)
    {
        update_antigravity_project(&mut *auth_store.lock().await, &discovered);
        return Some((token, discovered));
    }

    Some((token, fallback_antigravity_project(current_id)))
}

async fn fetch_antigravity_quota_with_retry(
    auth_store: &tokio::sync::Mutex<AuthStore>,
    token: &str,
    project_id: &str,
    target_model: &str,
) -> Option<String> {
    if let Some(display) = crate::antigravity::fetch_quota(token, project_id, target_model).await {
        return Some(display);
    }
    auth_store.lock().await.force_refresh("antigravity").await.ok()?;
    let (fresh_token, fresh_project) = resolve_antigravity_credentials(auth_store).await?;
    crate::antigravity::fetch_quota(&fresh_token, &fresh_project, target_model).await
}

async fn do_refresh_antigravity_quota(
    auth_store: Arc<tokio::sync::Mutex<AuthStore>>,
    quota: QuotaTracker,
    target_model: String,
) {
    let key = QuotaKey::new("antigravity", Some(&target_model));
    if !quota.begin_fetch(&key) {
        return;
    }
    let Some((token, project_id)) = resolve_antigravity_credentials(&auth_store).await else {
        quota.record_failure(&key);
        return;
    };
    let display = fetch_antigravity_quota_with_retry(&auth_store, &token, &project_id, &target_model).await;
    record_quota_result(&quota, &key, display);
}

async fn resolve_chatgpt_credentials(auth_store: &tokio::sync::Mutex<AuthStore>) -> Option<(String, Option<String>)> {
    let mut store = auth_store.lock().await;
    let token = store.get_key("chatgpt").await.ok().flatten()?;
    let account_id = stored_oauth_account_id(store.get_credential("chatgpt"))
        .or_else(|| crate::auth::oauth::extract_chatgpt_account_id(&token));
    Some((token, account_id))
}

async fn fetch_chatgpt_quota_with_retry(
    auth_store: &tokio::sync::Mutex<AuthStore>,
    token: &str,
    account_id: Option<&str>,
) -> Option<String> {
    if let Some(display) = crate::chatgpt::fetch_quota(token, account_id).await {
        return Some(display);
    }
    auth_store.lock().await.force_refresh("chatgpt").await.ok()?;
    let (fresh_token, fresh_account) = resolve_chatgpt_credentials(auth_store).await?;
    crate::chatgpt::fetch_quota(&fresh_token, fresh_account.as_deref()).await
}

async fn do_refresh_chatgpt_quota(auth_store: Arc<tokio::sync::Mutex<AuthStore>>, quota: QuotaTracker) {
    let key = QuotaKey::new("chatgpt", None::<String>);
    if !quota.begin_fetch(&key) {
        return;
    }
    let Some((token, account_id)) = resolve_chatgpt_credentials(&auth_store).await else {
        quota.record_failure(&key);
        return;
    };
    let display = fetch_chatgpt_quota_with_retry(&auth_store, &token, account_id.as_deref()).await;
    record_quota_result(&quota, &key, display);
}

async fn fetch_claude_quota_with_retry(
    auth_store: &tokio::sync::Mutex<AuthStore>,
    token: &str,
    target_model: &str,
) -> Option<String> {
    if let Some(display) = crate::claude::quota::fetch_quota(token, Some(target_model)).await {
        return Some(display);
    }
    let fresh_token = auth_store.lock().await.force_refresh("claude").await.ok()??;
    crate::claude::quota::fetch_quota(&fresh_token, Some(target_model)).await
}

async fn do_refresh_claude_quota(
    auth_store: Arc<tokio::sync::Mutex<AuthStore>>,
    quota: QuotaTracker,
    target_model: String,
) {
    let key = QuotaKey::new("claude", Some(&target_model));
    if !quota.begin_fetch(&key) {
        return;
    }
    let token = auth_store.lock().await.get_key("claude").await.ok().flatten();
    let Some(token) = token else {
        quota.record_failure(&key);
        return;
    };
    let display = fetch_claude_quota_with_retry(&auth_store, &token, &target_model).await;
    record_quota_result(&quota, &key, display);
}

async fn do_refresh_ollama_quota(auth_store: Arc<tokio::sync::Mutex<AuthStore>>, quota: QuotaTracker) {
    let key = QuotaKey::new("ollama-cloud", None::<String>);
    if !quota.begin_fetch(&key) {
        return;
    }
    let token = auth_store.lock().await.get_key("ollama-cloud").await.ok().flatten();
    let Some(token) = token else {
        quota.record_failure(&key);
        return;
    };
    let display = crate::ollama::fetch_quota(&token).await;
    record_quota_result(&quota, &key, display);
}

async fn do_refresh_gemini_quota(
    sessions_dir: &std::path::Path,
    current_session_id: Option<&str>,
    totals: crate::engine::SessionUsageTotals,
    target_model: &str,
    quota: QuotaTracker,
) {
    let key = QuotaKey::new("gemini", Some(target_model));
    if !quota.begin_fetch(&key) {
        return;
    }
    let display = crate::gemini::fetch_quota(sessions_dir, current_session_id, &totals, target_model).await;
    record_quota_result(&quota, &key, display);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_project_id_detection() {
        assert!(is_synthetic_project_id(""));
        assert!(is_synthetic_project_id("   "));
        let uuid_id = crate::auth::antigravity::stable_project_id("user@example.com");
        assert!(is_synthetic_project_id(&uuid_id));
        assert!(!is_synthetic_project_id("earnest-shoreline-k4tm7"));
        assert!(!is_synthetic_project_id("my-gcp-project-123"));
    }

    #[test]
    fn stored_oauth_account_id_extraction() {
        assert_eq!(stored_oauth_account_id(None), None);
        let api_cred = StoredCredential::api_key("sk-test");
        assert_eq!(stored_oauth_account_id(Some(&api_cred)), None);

        let oauth_with_id = StoredCredential::OAuth {
            access_token: "tok".into(),
            refresh_token: None,
            expires_at_ms: None,
            account_id: Some("proj-abc".into()),
            account_email: None,
        };
        assert_eq!(stored_oauth_account_id(Some(&oauth_with_id)), Some("proj-abc".into()));

        let oauth_no_id = StoredCredential::OAuth {
            access_token: "tok".into(),
            refresh_token: None,
            expires_at_ms: None,
            account_id: None,
            account_email: None,
        };
        assert_eq!(stored_oauth_account_id(Some(&oauth_no_id)), None);
    }

    #[test]
    fn fallback_antigravity_project_handling() {
        let default_proj = crate::auth::antigravity::stable_project_id("antigravity-default");
        assert_eq!(fallback_antigravity_project(String::new()), default_proj);
        assert_eq!(
            fallback_antigravity_project("concrete-project".into()),
            "concrete-project"
        );
    }

    #[test]
    fn update_antigravity_project_updates_oauth_credential() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = AuthStore::load(temp.path().join("auth.json")).unwrap();
        update_antigravity_project(&mut store, "discovered-proj");

        let oauth_cred = StoredCredential::OAuth {
            access_token: "tok".into(),
            refresh_token: None,
            expires_at_ms: None,
            account_id: None,
            account_email: None,
        };
        store.set_credential("antigravity", oauth_cred).unwrap();
        update_antigravity_project(&mut store, "discovered-proj");

        assert_eq!(
            stored_oauth_account_id(store.get_credential("antigravity")),
            Some("discovered-proj".into())
        );
    }

    #[test]
    fn record_quota_result_updates_tracker() {
        let tracker = QuotaTracker::default();
        let key = QuotaKey::new("antigravity", Some("gemini-2.5-flash"));
        record_quota_result(&tracker, &key, Some("80% (2h)".into()));
        assert_eq!(tracker.display_for(&key), Some("80% (2h)".into()));

        let failed_key = QuotaKey::new("antigravity", Some("gemini-2.5-pro"));
        record_quota_result(&tracker, &failed_key, None);
        assert_eq!(tracker.display_for(&failed_key), None);
    }

    #[tokio::test]
    async fn resolve_antigravity_credentials_scenarios() {
        let temp = tempfile::tempdir().unwrap();
        let store = tokio::sync::Mutex::new(AuthStore::load(temp.path().join("auth.json")).unwrap());
        assert_eq!(resolve_antigravity_credentials(&store).await, None);

        let concrete_cred = StoredCredential::OAuth {
            access_token: "tok-123".into(),
            refresh_token: None,
            expires_at_ms: None,
            account_id: Some("prod-gcp-project".into()),
            account_email: None,
        };
        store.lock().await.set_credential("antigravity", concrete_cred).unwrap();
        assert_eq!(
            resolve_antigravity_credentials(&store).await,
            Some(("tok-123".into(), "prod-gcp-project".into()))
        );

        let empty_id_cred = StoredCredential::OAuth {
            access_token: "tok-empty".into(),
            refresh_token: None,
            expires_at_ms: None,
            account_id: None,
            account_email: None,
        };
        store.lock().await.set_credential("antigravity", empty_id_cred).unwrap();
        let default_proj = crate::auth::antigravity::stable_project_id("antigravity-default");
        assert_eq!(
            resolve_antigravity_credentials(&store).await,
            Some(("tok-empty".into(), default_proj))
        );
    }

    #[tokio::test]
    async fn resolve_chatgpt_credentials_scenarios() {
        let temp = tempfile::tempdir().unwrap();
        let store = tokio::sync::Mutex::new(AuthStore::load(temp.path().join("auth.json")).unwrap());
        assert_eq!(resolve_chatgpt_credentials(&store).await, None);

        let oauth_cred = StoredCredential::OAuth {
            access_token: "chatgpt-tok".into(),
            refresh_token: None,
            expires_at_ms: None,
            account_id: Some("acct-456".into()),
            account_email: None,
        };
        store.lock().await.set_credential("chatgpt", oauth_cred).unwrap();
        assert_eq!(
            resolve_chatgpt_credentials(&store).await,
            Some(("chatgpt-tok".into(), Some("acct-456".into())))
        );

        let api_cred = StoredCredential::api_key("sk-non-jwt");
        store.lock().await.set_credential("chatgpt", api_cred).unwrap();
        assert_eq!(
            resolve_chatgpt_credentials(&store).await,
            Some(("sk-non-jwt".into(), None))
        );
    }

    #[tokio::test]
    async fn do_refresh_antigravity_quota_branches() {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(tokio::sync::Mutex::new(
            AuthStore::load(temp.path().join("auth.json")).unwrap(),
        ));
        let quota = QuotaTracker::default();
        let key = QuotaKey::new("antigravity", Some("gemini-2.5-pro"));

        quota.begin_fetch(&key);
        do_refresh_antigravity_quota(Arc::clone(&store), quota.clone(), "gemini-2.5-pro".into()).await;
        quota.record_failure(&key);

        do_refresh_antigravity_quota(Arc::clone(&store), quota.clone(), "gemini-2.5-pro".into()).await;
        assert_eq!(quota.display_for(&key), None);

        quota.invalidate(&key);
        let cred = StoredCredential::OAuth {
            access_token: "invalid-token".into(),
            refresh_token: None,
            expires_at_ms: None,
            account_id: Some("custom-proj".into()),
            account_email: None,
        };
        store.lock().await.set_credential("antigravity", cred).unwrap();
        do_refresh_antigravity_quota(store, quota.clone(), "gemini-2.5-pro".into()).await;
        assert_eq!(quota.display_for(&key), None);
    }

    #[tokio::test]
    async fn do_refresh_chatgpt_quota_branches() {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(tokio::sync::Mutex::new(
            AuthStore::load(temp.path().join("auth.json")).unwrap(),
        ));
        let quota = QuotaTracker::default();
        let key = QuotaKey::new("chatgpt", None::<String>);

        quota.begin_fetch(&key);
        do_refresh_chatgpt_quota(Arc::clone(&store), quota.clone()).await;
        quota.record_failure(&key);

        do_refresh_chatgpt_quota(Arc::clone(&store), quota.clone()).await;
        assert_eq!(quota.display_for(&key), None);

        quota.invalidate(&key);
        let cred = StoredCredential::api_key("invalid-token");
        store.lock().await.set_credential("chatgpt", cred).unwrap();
        do_refresh_chatgpt_quota(store, quota.clone()).await;
        assert_eq!(quota.display_for(&key), None);
    }

    #[tokio::test]
    async fn do_refresh_claude_quota_branches() {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(tokio::sync::Mutex::new(
            AuthStore::load(temp.path().join("auth.json")).unwrap(),
        ));
        let quota = QuotaTracker::default();
        let key = QuotaKey::new("claude", Some("claude-sonnet-4-6"));

        quota.begin_fetch(&key);
        do_refresh_claude_quota(Arc::clone(&store), quota.clone(), "claude-sonnet-4-6".into()).await;
        quota.record_failure(&key);

        do_refresh_claude_quota(Arc::clone(&store), quota.clone(), "claude-sonnet-4-6".into()).await;
        assert_eq!(quota.display_for(&key), None);

        quota.invalidate(&key);
        let cred = StoredCredential::api_key("invalid-token");
        store.lock().await.set_credential("claude", cred).unwrap();
        do_refresh_claude_quota(store, quota.clone(), "claude-sonnet-4-6".into()).await;
        assert_eq!(quota.display_for(&key), None);
    }

    #[tokio::test]
    async fn do_refresh_ollama_and_gemini_quota_branches() {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(tokio::sync::Mutex::new(
            AuthStore::load(temp.path().join("auth.json")).unwrap(),
        ));
        let quota = QuotaTracker::default();
        let ollama_key = QuotaKey::new("ollama-cloud", None::<String>);

        quota.begin_fetch(&ollama_key);
        do_refresh_ollama_quota(Arc::clone(&store), quota.clone()).await;
        quota.record_failure(&ollama_key);

        do_refresh_ollama_quota(store, quota.clone()).await;
        assert_eq!(quota.display_for(&ollama_key), None);

        let gemini_key = QuotaKey::new("gemini", Some("gemini-2.5-flash"));
        quota.begin_fetch(&gemini_key);
        do_refresh_gemini_quota(
            temp.path(),
            None,
            crate::engine::SessionUsageTotals::default(),
            "gemini-2.5-flash",
            quota.clone(),
        )
        .await;
    }

    #[tokio::test]
    async fn engine_quota_lifecycle_and_invalidation() {
        let temp = tempfile::tempdir().unwrap();
        let engine = crate::engine::eval::mock::mock_engine(
            rig::test_utils::MockCompletionModel::default(),
            crate::engine::eval::mock::MockEngineConfig {
                base_dir: temp.path(),
                app_config: rho_harness_core::config::Config {
                    provider: "antigravity".into(),
                    model: "gemini-2.5-flash".into(),
                    ..Default::default()
                },
                session_manager: None,
                built_in_tools: None,
            },
        );

        assert!(engine.should_refresh_quota());
        let mut rx = engine.quota_subscribe();
        assert_eq!(*rx.borrow_and_update(), 0);
        assert_eq!(engine.quota_version(), 0);

        engine.refresh_quota().await;
        engine.invalidate_quota();
        engine.force_refresh_quota();
        assert_eq!(engine.quota_display(), None);
    }

    #[tokio::test]
    async fn engine_refresh_quota_all_providers() {
        let temp = tempfile::tempdir().unwrap();
        for provider in &["chatgpt", "claude", "gemini", "ollama-cloud", "unknown"] {
            let engine = crate::engine::eval::mock::mock_engine(
                rig::test_utils::MockCompletionModel::default(),
                crate::engine::eval::mock::MockEngineConfig {
                    base_dir: temp.path(),
                    app_config: rho_harness_core::config::Config {
                        provider: (*provider).into(),
                        model: "test-model".into(),
                        ..Default::default()
                    },
                    session_manager: None,
                    built_in_tools: None,
                },
            );
            engine.refresh_quota().await;
            engine.spawn_refresh_quota();
        }
    }
}
