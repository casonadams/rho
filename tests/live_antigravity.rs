#![cfg(unix)]

//! Live verification of the Antigravity provider against Google's Cloud Code
//! Assist backend. Opt-in only: requires RHO_LIVE_ANTIGRAVITY=1 plus
//! RHO_LIVE_AG_ACCESS (OAuth access token), RHO_LIVE_AG_REFRESH, and
//! RHO_LIVE_AG_PROJECT. Kept out of CI because it spends real quota.

use crate::live_support::{LiveCheck, PlantedFact};
use rho_harness_core::auth::StoredCredential;

const ANTIGRAVITY: LiveCheck = LiveCheck {
    label: "Antigravity",
    provider: "antigravity",
    enable_var: "RHO_LIVE_ANTIGRAVITY",
    model_var: "RHO_LIVE_AG_MODEL",
    thinking_var: "RHO_LIVE_AG_THINKING",
};

fn live_credential() -> StoredCredential {
    // Seed the token already expired (unless RHO_LIVE_AG_FRESH=1) so the run
    // also proves the engine's startup auto-refresh path.
    let expires_at_ms = if std::env::var("RHO_LIVE_AG_FRESH").ok().as_deref() == Some("1") {
        chrono::Utc::now().timestamp_millis() + 30 * 60 * 1000
    } else {
        chrono::Utc::now().timestamp_millis() - 60 * 1000
    };
    StoredCredential::oauth(
        ANTIGRAVITY.env("RHO_LIVE_AG_ACCESS"),
        Some(ANTIGRAVITY.env("RHO_LIVE_AG_REFRESH")),
        Some(expires_at_ms),
    )
}

#[tokio::test]
async fn live_antigravity_dynamic_model_discovery_lists_runtime_catalog() {
    if !ANTIGRAVITY.enabled() {
        return;
    }

    let access = ANTIGRAVITY.env("RHO_LIVE_AG_ACCESS");
    let project = ANTIGRAVITY.env("RHO_LIVE_AG_PROJECT");
    let models = rho_engine::antigravity::discover_models(&access, &project)
        .await
        .expect("live fetchAvailableModels discovery");

    assert!(
        models.iter().any(|id| id.starts_with("gemini-3.8-flash")),
        "gemini-3.8-flash family missing from live catalog: {models:?}"
    );
    assert!(
        models.iter().any(|id| id.starts_with("claude-")),
        "claude models missing from live catalog: {models:?}"
    );
    assert!(
        models
            .iter()
            .all(|id| !id.starts_with("chat_") && !id.starts_with("tab_")),
        "non-agent models leaked into catalog: {models:?}"
    );
}

#[tokio::test]
async fn live_antigravity_multi_turn_session_recalls_planted_fact() {
    if !ANTIGRAVITY.enabled() {
        return;
    }
    let fact = PlantedFact {
        plant_prompt: "My favorite color is chartreuse. Reply with exactly: OK-CHARTREUSE-RECEIVED",
        acknowledgement: "OK-CHARTREUSE-RECEIVED",
        recall_prompt: "What is my favorite color? Answer with only the color name, nothing else.",
        answer: "chartreuse",
    };
    ANTIGRAVITY.assert_session_recalls(live_credential(), &fact).await;
}
