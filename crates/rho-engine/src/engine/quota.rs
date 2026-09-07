use std::sync::Arc;

use rho_harness_core::auth::StoredCredential;

use super::AgentEngine;
use crate::auth::AuthStore;
use crate::engine::tracking::{QuotaKey, QuotaTracker};

pub(crate) fn canonical_quota_provider(provider: &str) -> Option<&'static str> {
    let trimmed = provider.trim();
    if trimmed.eq_ignore_ascii_case("ollama-cloud") {
        Some("ollama-cloud")
    } else if trimmed.eq_ignore_ascii_case("antigravity") || trimmed.eq_ignore_ascii_case("google-antigravity") {
        Some("antigravity")
    } else if trimmed.eq_ignore_ascii_case("chatgpt") || trimmed.eq_ignore_ascii_case("openai-chatgpt") {
        Some("chatgpt")
    } else {
        None
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
}

async fn do_refresh_ollama_quota(auth_store: Arc<tokio::sync::Mutex<AuthStore>>, quota: QuotaTracker) {
    let key = QuotaKey::new("ollama-cloud", None::<String>);
    if !quota.should_fetch(&key) {
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

async fn resolve_antigravity_credentials(auth_store: &tokio::sync::Mutex<AuthStore>) -> Option<(String, String)> {
    let mut store = auth_store.lock().await;
    let token = store.get_key("antigravity").await.ok().flatten()?;
    let project_id = match store.get_credential("antigravity") {
        Some(StoredCredential::OAuth {
            account_id: Some(id), ..
        }) => id.clone(),
        _ => crate::auth::antigravity::stable_project_id("antigravity-default"),
    };
    Some((token, project_id))
}

async fn do_refresh_antigravity_quota(
    auth_store: Arc<tokio::sync::Mutex<AuthStore>>,
    quota: QuotaTracker,
    target_model: String,
) {
    let key = QuotaKey::new("antigravity", Some(&target_model));
    if !quota.should_fetch(&key) {
        return;
    }
    let Some((token, project_id)) = resolve_antigravity_credentials(&auth_store).await else {
        quota.record_failure(&key);
        return;
    };
    match crate::antigravity::fetch_quota(&token, &project_id, &target_model).await {
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
    if !quota.should_fetch(&key) {
        return;
    }
    let Some((token, account_id)) = resolve_chatgpt_credentials(&auth_store).await else {
        quota.record_failure(&key);
        return;
    };
    match crate::chatgpt::fetch_quota(&token, account_id.as_deref()).await {
        Some(display) => quota.record_success(&key, display),
        None => quota.record_failure(&key),
    }
}
