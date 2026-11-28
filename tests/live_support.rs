#![cfg(unix)]

use crate::temp_dir;
use rho::auth::AuthStore;
use rho::config::Config;
use rho::engine::AgentEngine;
use rho::engine::metrics::TerminalStatus;
use rho::engine::runner::TurnRequest;
use rho::presentation::{RecordingSink, StructuredPresenter};
use rho_harness_core::auth::StoredCredential;
use rho_harness_core::presentation::Presenter;
use std::sync::Arc;

pub(crate) struct LiveCheck {
    pub label: &'static str,
    pub provider: &'static str,
    pub enable_var: &'static str,
    pub model_var: &'static str,
    pub thinking_var: &'static str,
}

pub(crate) struct PlantedFact {
    pub plant_prompt: &'static str,
    pub acknowledgement: &'static str,
    pub recall_prompt: &'static str,
    pub answer: &'static str,
}

impl LiveCheck {
    pub fn enabled(&self) -> bool {
        let enabled = std::env::var(self.enable_var).ok().as_deref() == Some("1");
        if !enabled {
            eprintln!(
                "skipping: set {}=1 to run the live {} check",
                self.enable_var, self.label
            );
        }
        enabled
    }

    pub fn env(&self, name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for the live {} check", self.label))
    }

    fn config(&self, workspace: &std::path::Path) -> Config {
        Config {
            provider: self.provider.to_string(),
            model: std::env::var(self.model_var).unwrap_or_else(|_| "claude-sonnet-4-6".to_string()),
            auth_file: workspace.join("auth.json"),
            sessions_dir: workspace.join("sessions"),
            max_turns: 4,
            thinking_level: std::env::var(self.thinking_var).ok(),
            ..Config::default()
        }
    }

    pub async fn assert_session_recalls(&self, credential: StoredCredential, fact: &PlantedFact) {
        let workspace = temp_dir(&format!("rho_live_{}", self.provider));
        let config = self.config(&workspace);
        let mut auth_store = AuthStore::load(&config.auth_file).unwrap();
        auth_store.set_credential(self.provider, credential).unwrap();
        let engine = AgentEngine::new(config, auth_store, None).await.unwrap();
        let presenter: Arc<dyn Presenter> = Arc::new(StructuredPresenter::recording(RecordingSink::default()));
        let acknowledgement = completed_reply(&engine, presenter.clone(), fact.plant_prompt).await;
        assert!(acknowledgement.contains(fact.acknowledgement), "{acknowledgement}");
        let recall = completed_reply(&engine, presenter, fact.recall_prompt).await;
        assert!(recall.to_lowercase().contains(&fact.answer.to_lowercase()), "{recall}");
    }
}

async fn completed_reply(engine: &AgentEngine, presenter: Arc<dyn Presenter>, prompt: &str) -> String {
    let outcome = engine.run_turn(TurnRequest::new(prompt), presenter).await.unwrap();
    assert_eq!(outcome.status, TerminalStatus::Completed);
    outcome.final_text
}
