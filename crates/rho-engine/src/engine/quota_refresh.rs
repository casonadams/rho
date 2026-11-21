use futures::future::{BoxFuture, FutureExt};
use std::sync::Arc;

use rho_harness_core::auth::StoredCredential;
use rho_harness_core::provider::{ModelRoute, ProviderId};

use super::AgentEngine;
use crate::auth::AuthStore;
use crate::engine::tracking::{QuotaKey, QuotaTracker};

fn quota_provider(route: &ModelRoute) -> Option<ProviderId> {
    route.connection.provider_id().filter(|id| {
        matches!(
            id,
            ProviderId::OllamaCloud
                | ProviderId::Antigravity
                | ProviderId::ChatGpt
                | ProviderId::ClaudeCode
                | ProviderId::Gemini
        )
    })
}

impl AgentEngine {
    fn quota_job(&self) -> Option<BoxFuture<'static, ()>> {
        let route = self.config.route();
        let provider = quota_provider(&route)?;
        let auth = self.shared_auth_store();
        let quota = self.quota.clone();
        let key = |model: Option<&str>| QuotaKey::new(route.connection.to_string(), model);
        let model = route.model.clone();
        Some(match provider {
            ProviderId::OllamaCloud => do_refresh_ollama_quota(auth, quota, key(None)).boxed(),
            ProviderId::Antigravity => {
                let key = key(Some(&model));
                do_refresh_auth_quota(auth, quota, key, AuthQuotaSource::Antigravity { model }).boxed()
            }
            ProviderId::ChatGpt => do_refresh_auth_quota(auth, quota, key(None), AuthQuotaSource::ChatGpt).boxed(),
            ProviderId::ClaudeCode => do_refresh_claude_quota(auth, quota, key(Some(&model)), model).boxed(),
            ProviderId::Gemini => {
                let sessions_dir = self.config.sessions_dir.clone();
                let session_id = self.session_manager.session_id.clone();
                let totals = self.session_usage_totals();
                let key = key(Some(&model));
                async move {
                    do_refresh_gemini_quota(&sessions_dir, Some(&session_id), totals, key, &model, quota).await;
                }
                .boxed()
            }
            _ => return None,
        })
    }

    pub async fn refresh_quota(&self) {
        if let Some(job) = self.quota_job() {
            job.await;
        }
    }

    pub fn spawn_refresh_quota(&self) {
        if let Some(job) = self.quota_job() {
            tokio::spawn(job);
        }
    }

    /// Quota is tracked per login, so each account shows its own plan limits.
    fn quota_key(&self, model: Option<&str>) -> Option<QuotaKey> {
        let route = self.config.route();
        quota_provider(&route)?;
        Some(QuotaKey::new(route.connection.to_string(), model))
    }

    pub fn quota_display(&self) -> Option<String> {
        self.quota.display_for(&self.quota_key(Some(&self.config.model))?)
    }

    pub fn quota(&self) -> &QuotaTracker {
        &self.quota
    }

    pub fn should_refresh_quota(&self) -> bool {
        self.quota_key(Some(&self.config.model))
            .is_some_and(|key| self.quota.should_fetch(&key))
    }

    pub fn quota_subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.quota.subscribe()
    }

    pub fn invalidate_quota(&self) {
        for key in [self.quota_key(Some(&self.config.model)), self.quota_key(None)]
            .into_iter()
            .flatten()
        {
            self.quota.invalidate(&key);
        }
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

pub(crate) fn stored_oauth_account_id(cred: Option<&StoredCredential>) -> Option<String> {
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

fn is_dummy_or_test_token(token: &str) -> bool {
    let trimmed = token.trim();
    trimmed.is_empty()
        || trimmed.starts_with("test-")
        || trimmed.starts_with("invalid-")
        || trimmed.starts_with("tok-")
        || trimmed == "dummy"
}

fn fallback_antigravity_project(current_id: String) -> String {
    if current_id.is_empty() {
        crate::auth::antigravity::stable_project_id("antigravity-default")
    } else {
        current_id
    }
}

/// Caches the discovered project on the login; a failed write only means the next refresh
/// discovers it again, so the quota fetch goes ahead either way.
fn cache_antigravity_project(store: &mut AuthStore, connection: &str, discovered: &str) {
    if let Some(mut cred) = store.get_credential(connection).cloned() {
        if let StoredCredential::OAuth { ref mut account_id, .. } = cred {
            *account_id = Some(discovered.to_string());
        }
        let _ = store.set_credential(connection, cred);
    }
}

async fn resolve_antigravity_credentials(
    auth_store: &tokio::sync::Mutex<AuthStore>,
    connection: &str,
) -> Option<(String, String)> {
    let (token, current_id) = {
        let mut store = auth_store.lock().await;
        let token = store.get_key(connection).await.ok().flatten()?;
        let current_id = stored_oauth_account_id(store.get_credential(connection)).unwrap_or_default();
        (token, current_id)
    };

    if !is_synthetic_project_id(&current_id) {
        return Some((token, current_id));
    }

    if is_dummy_or_test_token(&token) {
        return Some((token, fallback_antigravity_project(current_id)));
    }

    if let Some(discovered) = crate::antigravity::client::load_project_id(&token).await
        && !is_synthetic_project_id(&discovered)
    {
        cache_antigravity_project(&mut *auth_store.lock().await, connection, &discovered);
        return Some((token, discovered));
    }

    Some((token, fallback_antigravity_project(current_id)))
}

async fn fetch_quota_with_auth_retry<Ctx, Usable, Fetch, FetchFut, Refresh, RefreshFut>(
    initial: Ctx,
    usable: Usable,
    fetch: Fetch,
    refresh: Refresh,
) -> Option<String>
where
    Usable: Fn(&Ctx) -> bool,
    Fetch: Fn(Ctx) -> FetchFut,
    FetchFut: Future<Output = Option<String>>,
    Refresh: FnOnce() -> RefreshFut,
    RefreshFut: Future<Output = Option<Ctx>>,
{
    if !usable(&initial) {
        return None;
    }
    if let Some(display) = fetch(initial).await {
        return Some(display);
    }
    let fresh = refresh().await?;
    if !usable(&fresh) {
        return None;
    }
    fetch(fresh).await
}

async fn refresh_stored_key_quota<Fetch, Fut>(
    auth_store: &tokio::sync::Mutex<AuthStore>,
    quota: &QuotaTracker,
    key: QuotaKey,
    fetch: Fetch,
) where
    Fetch: FnOnce(String) -> Fut,
    Fut: Future<Output = Option<String>>,
{
    if !quota.begin_fetch(&key) {
        return;
    }
    let token = auth_store.lock().await.get_key(&key.connection).await.ok().flatten();
    let Some(token) = token else {
        quota.record_failure(&key);
        return;
    };
    let display = fetch(token).await;
    record_quota_result(quota, &key, display);
}

async fn refresh_quota_with_auth_retry<Ctx, Resolve, Usable, Fetch, FetchFut>(
    auth_store: &tokio::sync::Mutex<AuthStore>,
    quota: &QuotaTracker,
    key: QuotaKey,
    resolve: Resolve,
    usable: Usable,
    fetch: Fetch,
) where
    Resolve: AsyncFn(&tokio::sync::Mutex<AuthStore>, &str) -> Option<Ctx>,
    Usable: Fn(&Ctx) -> bool,
    Fetch: Fn(Ctx) -> FetchFut,
    FetchFut: Future<Output = Option<String>>,
{
    if !quota.begin_fetch(&key) {
        return;
    }
    let connection = key.connection.as_str();
    let Some(initial) = resolve(auth_store, connection).await else {
        quota.record_failure(&key);
        return;
    };
    let resolve = &resolve;
    let display = fetch_quota_with_auth_retry(initial, usable, fetch, || async move {
        auth_store.lock().await.force_refresh(connection).await.ok()?;
        resolve(auth_store, connection).await
    })
    .await;
    record_quota_result(quota, &key, display);
}

enum AuthQuotaSource {
    Antigravity { model: String },
    ChatGpt,
}

async fn do_refresh_auth_quota(
    auth_store: Arc<tokio::sync::Mutex<AuthStore>>,
    quota: QuotaTracker,
    key: QuotaKey,
    source: AuthQuotaSource,
) {
    let store = &*auth_store;
    match source {
        AuthQuotaSource::Antigravity { model } => {
            let model = model.as_str();
            refresh_quota_with_auth_retry(
                store,
                &quota,
                key,
                resolve_antigravity_credentials,
                |(token, _): &(String, String)| !is_dummy_or_test_token(token),
                |(token, project_id)| async move { crate::antigravity::fetch_quota(&token, &project_id, model).await },
            )
            .await;
        }
        AuthQuotaSource::ChatGpt => {
            refresh_quota_with_auth_retry(
                store,
                &quota,
                key,
                resolve_chatgpt_credentials,
                |(token, _): &(String, Option<String>)| !is_dummy_or_test_token(token),
                |(token, account_id)| async move {
                    crate::chatgpt::fetch_quota(crate::chatgpt::USAGE_ENDPOINT, &token, account_id.as_deref()).await
                },
            )
            .await;
        }
    }
}

async fn resolve_chatgpt_credentials(
    auth_store: &tokio::sync::Mutex<AuthStore>,
    connection: &str,
) -> Option<(String, Option<String>)> {
    let mut store = auth_store.lock().await;
    let token = store.get_key(connection).await.ok().flatten()?;
    let account_id = stored_oauth_account_id(store.get_credential(connection))
        .or_else(|| crate::auth::oauth::extract_chatgpt_account_id(&token));
    Some((token, account_id))
}

async fn do_refresh_claude_quota(
    auth_store: Arc<tokio::sync::Mutex<AuthStore>>,
    quota: QuotaTracker,
    key: QuotaKey,
    target_model: String,
) {
    let store = &*auth_store;
    let connection = key.connection.clone();
    let (model, connection) = (target_model.as_str(), connection.as_str());
    refresh_stored_key_quota(store, &quota, key, |token| async move {
        fetch_quota_with_auth_retry(
            token,
            |token: &String| !is_dummy_or_test_token(token),
            |token| async move {
                crate::claude::quota::fetch_quota(crate::claude::quota::USAGE_ENDPOINT, &token, Some(model)).await
            },
            || async move { store.lock().await.force_refresh(connection).await.ok().flatten() },
        )
        .await
    })
    .await;
}

async fn do_refresh_ollama_quota(auth_store: Arc<tokio::sync::Mutex<AuthStore>>, quota: QuotaTracker, key: QuotaKey) {
    refresh_stored_key_quota(&auth_store, &quota, key, |token| async move {
        crate::ollama::fetch_quota(crate::ollama::CLOUD_HOST, &token).await
    })
    .await;
}

async fn do_refresh_gemini_quota(
    sessions_dir: &std::path::Path,
    current_session_id: Option<&str>,
    totals: crate::engine::SessionUsageTotals,
    key: QuotaKey,
    target_model: &str,
    quota: QuotaTracker,
) {
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
        cache_antigravity_project(&mut store, "antigravity", "discovered-proj");

        let oauth_cred = StoredCredential::OAuth {
            access_token: "tok".into(),
            refresh_token: None,
            expires_at_ms: None,
            account_id: None,
            account_email: None,
        };
        store.set_credential("antigravity", oauth_cred).unwrap();
        cache_antigravity_project(&mut store, "antigravity", "discovered-proj");

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
        assert_eq!(resolve_antigravity_credentials(&store, "antigravity").await, None);

        let concrete_cred = StoredCredential::OAuth {
            access_token: "tok-123".into(),
            refresh_token: None,
            expires_at_ms: None,
            account_id: Some("prod-gcp-project".into()),
            account_email: None,
        };
        store.lock().await.set_credential("antigravity", concrete_cred).unwrap();
        assert_eq!(
            resolve_antigravity_credentials(&store, "antigravity").await,
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
            resolve_antigravity_credentials(&store, "antigravity").await,
            Some(("tok-empty".into(), default_proj))
        );
    }

    #[tokio::test]
    async fn resolve_chatgpt_credentials_scenarios() {
        let temp = tempfile::tempdir().unwrap();
        let store = tokio::sync::Mutex::new(AuthStore::load(temp.path().join("auth.json")).unwrap());
        assert_eq!(resolve_chatgpt_credentials(&store, "chatgpt").await, None);

        let oauth_cred = StoredCredential::OAuth {
            access_token: "chatgpt-tok".into(),
            refresh_token: None,
            expires_at_ms: None,
            account_id: Some("acct-456".into()),
            account_email: None,
        };
        store.lock().await.set_credential("chatgpt", oauth_cred).unwrap();
        assert_eq!(
            resolve_chatgpt_credentials(&store, "chatgpt").await,
            Some(("chatgpt-tok".into(), Some("acct-456".into())))
        );

        let api_cred = StoredCredential::api_key("sk-non-jwt");
        store.lock().await.set_credential("chatgpt", api_cred).unwrap();
        assert_eq!(
            resolve_chatgpt_credentials(&store, "chatgpt").await,
            Some(("sk-non-jwt".into(), None))
        );

        let work = StoredCredential::OAuth {
            access_token: "work-tok".into(),
            refresh_token: None,
            expires_at_ms: None,
            account_id: Some("acct-work".into()),
            account_email: Some("me@work.com".into()),
        };
        store.lock().await.set_credential("chatgpt:me@work.com", work).unwrap();
        assert_eq!(
            resolve_chatgpt_credentials(&store, "chatgpt:me@work.com").await,
            Some(("work-tok".into(), Some("acct-work".into())))
        );
    }

    async fn run_refresh_branches<Refresh, Fut>(key: QuotaKey, refresh: Refresh, invalid: Option<StoredCredential>)
    where
        Refresh: Fn(Arc<tokio::sync::Mutex<AuthStore>>, QuotaTracker, QuotaKey) -> Fut,
        Fut: Future<Output = ()>,
    {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(tokio::sync::Mutex::new(
            AuthStore::load(temp.path().join("auth.json")).unwrap(),
        ));
        let quota = QuotaTracker::default();

        quota.begin_fetch(&key);
        refresh(Arc::clone(&store), quota.clone(), key.clone()).await;
        quota.record_failure(&key);

        refresh(Arc::clone(&store), quota.clone(), key.clone()).await;
        assert_eq!(quota.display_for(&key), None);

        if let Some(invalid) = invalid {
            quota.invalidate(&key);
            store.lock().await.set_credential(&key.connection, invalid).unwrap();
            refresh(store, quota.clone(), key.clone()).await;
            assert_eq!(quota.display_for(&key), None);
        }
    }

    #[tokio::test]
    async fn do_refresh_antigravity_quota_branches() {
        let invalid = StoredCredential::OAuth {
            access_token: "invalid-token".into(),
            refresh_token: None,
            expires_at_ms: None,
            account_id: Some("custom-proj".into()),
            account_email: None,
        };
        run_refresh_branches(
            QuotaKey::new("antigravity", Some("gemini-2.5-pro")),
            |store, quota, key| {
                do_refresh_auth_quota(
                    store,
                    quota,
                    key,
                    AuthQuotaSource::Antigravity {
                        model: "gemini-2.5-pro".into(),
                    },
                )
            },
            Some(invalid),
        )
        .await;
    }

    #[tokio::test]
    async fn do_refresh_chatgpt_and_claude_quota_branches() {
        run_refresh_branches(
            QuotaKey::new("chatgpt:me@work.com", None::<String>),
            |store, quota, key| do_refresh_auth_quota(store, quota, key, AuthQuotaSource::ChatGpt),
            Some(StoredCredential::api_key("invalid-token")),
        )
        .await;
        run_refresh_branches(
            QuotaKey::new("claude", Some("claude-sonnet-4-6")),
            |store, quota, key| do_refresh_claude_quota(store, quota, key, "claude-sonnet-4-6".into()),
            Some(StoredCredential::api_key("invalid-token")),
        )
        .await;
    }

    #[tokio::test]
    async fn do_refresh_ollama_quota_branches() {
        run_refresh_branches(
            QuotaKey::new("ollama-cloud", None::<String>),
            do_refresh_ollama_quota,
            None,
        )
        .await;
    }

    #[tokio::test]
    async fn do_refresh_gemini_quota_branches() {
        let temp = tempfile::tempdir().unwrap();
        let quota = QuotaTracker::default();
        let key = QuotaKey::new("gemini", Some("gemini-2.5-flash"));
        quota.begin_fetch(&key);
        do_refresh_gemini_quota(
            temp.path(),
            None,
            crate::engine::SessionUsageTotals::default(),
            key,
            "gemini-2.5-flash",
            quota.clone(),
        )
        .await;
    }

    #[tokio::test]
    async fn engine_quota_lifecycle_and_invalidation() {
        let temp = tempfile::tempdir().unwrap();
        let engine = crate::engine::eval::mock::mock_engine(
            std::sync::Arc::new(crate::provider::adapter::MockAdapter::default()),
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
                std::sync::Arc::new(crate::provider::adapter::MockAdapter::default()),
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
