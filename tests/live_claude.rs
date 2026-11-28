#![cfg(unix)]

use crate::live_support::{LiveCheck, PlantedFact};
use rho_harness_core::auth::StoredCredential;

const CLAUDE: LiveCheck = LiveCheck {
    label: "Claude",
    provider: "claude",
    enable_var: "RHO_LIVE_CLAUDE",
    model_var: "RHO_LIVE_CLAUDE_MODEL",
    thinking_var: "RHO_LIVE_CLAUDE_THINKING",
};

#[tokio::test]
async fn live_claude_multi_turn_session_recalls_planted_fact() {
    if !CLAUDE.enabled() {
        return;
    }
    let expires_at_ms = chrono::Utc::now().timestamp_millis() + 30 * 60 * 1000;
    let credential = StoredCredential::oauth(
        CLAUDE.env("RHO_LIVE_CLAUDE_ACCESS"),
        std::env::var("RHO_LIVE_CLAUDE_REFRESH").ok(),
        Some(expires_at_ms),
    );
    let fact = PlantedFact {
        plant_prompt: "My secret word is PHOENIX-99. Reply with exactly: OK-PHOENIX-RECEIVED",
        acknowledgement: "OK-PHOENIX-RECEIVED",
        recall_prompt: "What is my secret word? Answer with only the secret word, nothing else.",
        answer: "PHOENIX-99",
    };
    CLAUDE.assert_session_recalls(credential, &fact).await;
}
