#![cfg(unix)]

//! Live verification of the Antigravity provider against Google's Cloud Code
//! Assist backend. Opt-in only: requires RHO_LIVE_ANTIGRAVITY=1 plus
//! RHO_LIVE_AG_ACCESS (OAuth access token), RHO_LIVE_AG_REFRESH, and
//! RHO_LIVE_AG_PROJECT. Kept out of CI because it spends real quota.

use rho::auth::AuthStore;
use rho::config::Config;
use rho::engine::AgentEngine;
use rho::engine::runner::TurnRequest;
use rho::presentation::{RecordingSink, StructuredPresenter};
use rho_harness_core::auth::StoredCredential;
use std::path::PathBuf;
use std::sync::Arc;

fn live_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for the live Antigravity check"))
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{name}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn live_config(workspace: &std::path::Path) -> Config {
    Config {
        provider: "antigravity".to_string(),
        model: std::env::var("RHO_LIVE_AG_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".to_string()),
        auth_file: workspace.join("auth.json"),
        sessions_dir: workspace.join("sessions"),
        max_turns: 4,
        ..Config::default()
    }
}

fn seed_auth(config: &Config) -> AuthStore {
    // Seed the token already expired (unless RHO_LIVE_AG_FRESH=1) so the run
    // also proves the engine's startup auto-refresh path.
    let expires_at_ms = if std::env::var("RHO_LIVE_AG_FRESH").ok().as_deref() == Some("1") {
        chrono::Utc::now().timestamp_millis() + 30 * 60 * 1000
    } else {
        chrono::Utc::now().timestamp_millis() - 60 * 1000
    };
    let credential = StoredCredential::oauth(
        live_env("RHO_LIVE_AG_ACCESS"),
        Some(live_env("RHO_LIVE_AG_REFRESH")),
        Some(expires_at_ms),
    );
    let mut store = AuthStore::load(&config.auth_file).unwrap();
    store.set_credential("antigravity", credential).unwrap();
    store
}

#[tokio::test]
async fn live_antigravity_dynamic_model_discovery_lists_runtime_catalog() {
    if std::env::var("RHO_LIVE_ANTIGRAVITY").ok().as_deref() != Some("1") {
        eprintln!("skipping: set RHO_LIVE_ANTIGRAVITY=1 to run the live Antigravity check");
        return;
    }

    let access = live_env("RHO_LIVE_AG_ACCESS");
    let project = live_env("RHO_LIVE_AG_PROJECT");
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
    if std::env::var("RHO_LIVE_ANTIGRAVITY").ok().as_deref() != Some("1") {
        eprintln!("skipping: set RHO_LIVE_ANTIGRAVITY=1 to run the live Antigravity check");
        return;
    }

    let workspace = temp_dir("rho_live_antigravity");
    let mut config = live_config(&workspace);
    // RHO_LIVE_AG_THINKING=<level> exercises effort routing + thinking config.
    config.thinking_level = std::env::var("RHO_LIVE_AG_THINKING").ok().filter(|l| l != "off");
    let auth_store = seed_auth(&config);

    let engine = AgentEngine::new(config, auth_store, None).await.unwrap();
    let presenter: Arc<dyn rho_harness_core::presentation::Presenter> =
        Arc::new(StructuredPresenter::recording(RecordingSink::default()));

    let first = engine
        .run_turn(
            TurnRequest::new("My favorite color is chartreuse. Reply with exactly: OK-CHARTREUSE-RECEIVED"),
            presenter.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        first.status,
        rho::engine::runner::RunStatus::Completed,
        "turn 1: {}",
        first.final_text
    );
    assert!(
        first.final_text.contains("OK-CHARTREUSE-RECEIVED"),
        "turn 1 text: {}",
        first.final_text
    );

    let second = engine
        .run_turn(
            TurnRequest::new("What is my favorite color? Answer with only the color name, nothing else."),
            presenter,
        )
        .await
        .unwrap();
    assert_eq!(
        second.status,
        rho::engine::runner::RunStatus::Completed,
        "turn 2: {}",
        second.final_text
    );
    assert!(
        second.final_text.to_lowercase().contains("chartreuse"),
        "session continuity broken, turn 2 text: {}",
        second.final_text
    );
}
