#![cfg(unix)]

use rho::auth::AuthStore;
use rho::config::Config;
use rho::engine::AgentEngine;
use rho::engine::runner::TurnRequest;
use rho::presentation::{RecordingSink, StructuredPresenter};
use rho_harness_core::auth::StoredCredential;
use std::path::PathBuf;
use std::sync::Arc;

fn live_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for the live Claude check"))
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{name}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn live_config(workspace: &std::path::Path) -> Config {
    Config {
        provider: "claude".to_string(),
        model: std::env::var("RHO_LIVE_CLAUDE_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".to_string()),
        auth_file: workspace.join("auth.json"),
        sessions_dir: workspace.join("sessions"),
        max_turns: 4,
        ..Config::default()
    }
}

fn seed_auth(config: &Config) -> AuthStore {
    let expires_at_ms = chrono::Utc::now().timestamp_millis() + 30 * 60 * 1000;
    let credential = StoredCredential::oauth(
        live_env("RHO_LIVE_CLAUDE_ACCESS"),
        std::env::var("RHO_LIVE_CLAUDE_REFRESH").ok(),
        Some(expires_at_ms),
    );
    let mut store = AuthStore::load(&config.auth_file).unwrap();
    store.set_credential("claude", credential).unwrap();
    store
}

async fn run_plant_fact_turn(engine: &AgentEngine, presenter: Arc<dyn rho_harness_core::presentation::Presenter>) {
    let first = engine
        .run_turn(
            TurnRequest::new("My secret word is PHOENIX-99. Reply with exactly: OK-PHOENIX-RECEIVED"),
            presenter,
        )
        .await
        .unwrap();
    assert_eq!(first.status, rho::engine::runner::RunStatus::Completed);
    assert!(first.final_text.contains("OK-PHOENIX-RECEIVED"));
}

async fn run_recall_fact_turn(engine: &AgentEngine, presenter: Arc<dyn rho_harness_core::presentation::Presenter>) {
    let second = engine
        .run_turn(
            TurnRequest::new("What is my secret word? Answer with only the secret word, nothing else."),
            presenter,
        )
        .await
        .unwrap();
    assert_eq!(second.status, rho::engine::runner::RunStatus::Completed);
    assert!(second.final_text.to_uppercase().contains("PHOENIX-99"));
}

#[tokio::test]
async fn live_claude_multi_turn_session_recalls_planted_fact() {
    if std::env::var("RHO_LIVE_CLAUDE").ok().as_deref() != Some("1") {
        eprintln!("skipping: set RHO_LIVE_CLAUDE=1 to run the live Claude check");
        return;
    }

    let workspace = temp_dir("rho_live_claude");
    let mut config = live_config(&workspace);
    config.thinking_level = std::env::var("RHO_LIVE_CLAUDE_THINKING").ok().filter(|l| l != "off");
    let auth_store = seed_auth(&config);
    let engine = AgentEngine::new(config, auth_store, None).await.unwrap();
    let presenter: Arc<dyn rho_harness_core::presentation::Presenter> =
        Arc::new(StructuredPresenter::recording(RecordingSink::default()));
    run_plant_fact_turn(&engine, presenter.clone()).await;
    run_recall_fact_turn(&engine, presenter).await;
    let _ = std::fs::remove_dir_all(workspace);
}
