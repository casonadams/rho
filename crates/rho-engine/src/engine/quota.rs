use std::sync::Arc;

use rho_harness_core::auth::StoredCredential;

use super::AgentEngine;
use crate::auth::AuthStore;
use crate::engine::tracking::QuotaTracker;

impl AgentEngine {
    pub async fn refresh_quota(&self) {
        let provider = self.config.provider.trim();
        if provider.eq_ignore_ascii_case("ollama-cloud") {
            do_refresh_ollama_quota(Arc::clone(&self.auth_store), self.quota.clone()).await;
        } else if provider.eq_ignore_ascii_case("antigravity") || provider.eq_ignore_ascii_case("google-antigravity") {
            do_refresh_antigravity_quota(
                Arc::clone(&self.auth_store),
                self.quota.clone(),
                self.config.model.clone(),
            )
            .await;
        }
    }

    pub fn spawn_refresh_quota(&self) {
        let provider = self.config.provider.trim().to_string();
        if provider.eq_ignore_ascii_case("ollama-cloud") {
            let auth = Arc::clone(&self.auth_store);
            let quota = self.quota.clone();
            tokio::spawn(async move {
                do_refresh_ollama_quota(auth, quota).await;
            });
        } else if provider.eq_ignore_ascii_case("antigravity") || provider.eq_ignore_ascii_case("google-antigravity") {
            let auth = Arc::clone(&self.auth_store);
            let quota = self.quota.clone();
            let model = self.config.model.clone();
            tokio::spawn(async move {
                do_refresh_antigravity_quota(auth, quota, model).await;
            });
        }
    }

    pub fn quota_display(&self) -> Option<String> {
        self.quota.latest()
    }
}

async fn do_refresh_ollama_quota(auth_store: Arc<tokio::sync::Mutex<AuthStore>>, quota: QuotaTracker) {
    if !quota.should_fetch() {
        return;
    }
    let key = auth_store.lock().await.get_key("ollama-cloud").await.ok().flatten();
    let Some(key) = key else {
        quota.record_failure();
        return;
    };
    match crate::ollama::fetch_quota(&key).await {
        Some(display) => quota.record_success(display),
        None => quota.record_failure(),
    }
}

async fn do_refresh_antigravity_quota(
    auth_store: Arc<tokio::sync::Mutex<AuthStore>>,
    quota: QuotaTracker,
    target_model: String,
) {
    if !quota.should_fetch() {
        return;
    }
    let (token, project_id) = {
        let mut store = auth_store.lock().await;
        let token = match store.get_key("antigravity").await {
            Ok(Some(t)) => t,
            _ => {
                quota.record_failure();
                return;
            }
        };
        let project_id = match store.get_credential("antigravity") {
            Some(StoredCredential::OAuth {
                account_id: Some(id), ..
            }) => id.clone(),
            _ => crate::auth::antigravity::stable_project_id("antigravity-default"),
        };
        (token, project_id)
    };
    match crate::antigravity::fetch_quota(&token, &project_id, &target_model).await {
        Some(display) => quota.record_success(display),
        None => quota.record_failure(),
    }
}
