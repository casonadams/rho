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
    match crate::ollama::fetch_quota(&token).await {
        Some(display) => quota.record_success(&key, display),
        None => quota.record_failure(&key),
    }
}

fn is_synthetic_project_id(id: &str) -> bool {
    let trimmed = id.trim();
    trimmed.is_empty() || uuid::Uuid::parse_str(trimmed).is_ok()
}

async fn resolve_antigravity_credentials(auth_store: &tokio::sync::Mutex<AuthStore>) -> Option<(String, String)> {
    let (token, current_id) = {
        let mut store = auth_store.lock().await;
        let token = store.get_key("antigravity").await.ok().flatten()?;
        let current_id = match store.get_credential("antigravity") {
            Some(StoredCredential::OAuth {
                account_id: Some(id), ..
            }) => id.clone(),
            _ => String::new(),
        };
        (token, current_id)
    };

    if !is_synthetic_project_id(&current_id) {
        return Some((token, current_id));
    }

    if let Some(discovered) = crate::antigravity::client::load_project_id(&token).await
        && !is_synthetic_project_id(&discovered)
    {
        let mut store = auth_store.lock().await;
        if let Some(mut cred) = store.get_credential("antigravity").cloned() {
            if let StoredCredential::OAuth { ref mut account_id, .. } = cred {
                *account_id = Some(discovered.clone());
            }
            let _ = store.set_credential("antigravity", cred);
        }
        return Some((token, discovered));
    }

    let fallback = if current_id.is_empty() {
        crate::auth::antigravity::stable_project_id("antigravity-default")
    } else {
        current_id
    };
    Some((token, fallback))
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
    let display = match crate::antigravity::fetch_quota(&token, &project_id, &target_model).await {
        Some(display) => Some(display),
        None => {
            if auth_store.lock().await.force_refresh("antigravity").await.is_ok()
                && let Some((fresh_token, fresh_project)) = resolve_antigravity_credentials(&auth_store).await
            {
                crate::antigravity::fetch_quota(&fresh_token, &fresh_project, &target_model).await
            } else {
                None
            }
        }
    };
    match display {
        Some(display) => quota.record_success(&key, display),
        None => quota.record_failure(&key),
    }
}

async fn resolve_chatgpt_credentials(auth_store: &tokio::sync::Mutex<AuthStore>) -> Option<(String, Option<String>)> {
    let mut store = auth_store.lock().await;
    let token = store.get_key("chatgpt").await.ok().flatten()?;
    let account_id = match store.get_credential("chatgpt") {
        Some(StoredCredential::OAuth {
            account_id: Some(id), ..
        }) => Some(id.clone()),
        _ => crate::auth::oauth::extract_chatgpt_account_id(&token),
    };
    Some((token, account_id))
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
    let display = match crate::chatgpt::fetch_quota(&token, account_id.as_deref()).await {
        Some(display) => Some(display),
        None => {
            if auth_store.lock().await.force_refresh("chatgpt").await.is_ok()
                && let Some((fresh_token, fresh_account)) = resolve_chatgpt_credentials(&auth_store).await
            {
                crate::chatgpt::fetch_quota(&fresh_token, fresh_account.as_deref()).await
            } else {
                None
            }
        }
    };
    match display {
        Some(display) => quota.record_success(&key, display),
        None => quota.record_failure(&key),
    }
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
    let display = match crate::claude::quota::fetch_quota(&token, Some(&target_model)).await {
        Some(display) => Some(display),
        None => {
            if let Ok(Some(fresh_token)) = auth_store.lock().await.force_refresh("claude").await {
                crate::claude::quota::fetch_quota(&fresh_token, Some(&target_model)).await
            } else {
                None
            }
        }
    };
    match display {
        Some(display) => quota.record_success(&key, display),
        None => quota.record_failure(&key),
    }
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
    if let Some(display) = crate::gemini::fetch_quota(sessions_dir, current_session_id, &totals, target_model).await {
        quota.record_success(&key, display);
    } else {
        quota.record_failure(&key);
    }
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
}
