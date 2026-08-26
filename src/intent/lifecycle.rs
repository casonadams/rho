pub mod progress;

use super::IntentSpec;
use crate::error::Result;
pub use progress::{IntentDecision, IntentProgress, VerificationResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntentStatus {
    Active,
    Blocked,
    Ready,
    Completed,
    Paused,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntentState {
    pub spec: IntentSpec,
    pub workspace: String,
    pub session_id: String,
    pub status: IntentStatus,
    pub decisions: Vec<IntentDecision>,
    pub completed_outcomes: Vec<String>,
    pub verification: Vec<VerificationResult>,
    pub blocked_reason: Option<String>,
    #[serde(default)]
    pub progress_reported: bool,
    pub revision: u64,
}

impl IntentState {
    pub fn new(spec: IntentSpec, workspace: String, session_id: String) -> Self {
        Self {
            spec,
            workspace,
            session_id,
            status: IntentStatus::Active,
            decisions: Vec::new(),
            completed_outcomes: Vec::new(),
            verification: Vec::new(),
            blocked_reason: None,
            progress_reported: false,
            revision: 0,
        }
    }

    pub fn amend(&mut self, next: &IntentSpec) {
        self.spec.outcomes.retain(|outcome| !outcome.starts_with("Follow-up: "));
        for constraint in &next.constraints {
            if !self.spec.constraints.contains(constraint) {
                self.spec.constraints.push(constraint.clone());
            }
        }
        for verification in &next.verification {
            if !self.spec.verification.contains(verification) {
                self.spec.verification.push(verification.clone());
            }
        }
        if matches!(
            self.status,
            IntentStatus::Completed | IntentStatus::Blocked | IntentStatus::Ready | IntentStatus::Paused
        ) {
            self.status = IntentStatus::Active;
        }
        self.blocked_reason = None;
        self.progress_reported = false;
        self.spec.status = "active".to_string();
    }

    pub fn record_decision(&mut self, key: &str, value: &str) -> Result<()> {
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            return Err(crate::error::AppError::Intent(
                "Intent decisions require a key and value".to_string(),
            ));
        }
        if let Some(existing) = self.decisions.iter_mut().find(|decision| decision.key == key) {
            if existing.value == value {
                return Ok(());
            }
            let previous = format!("{key}: {}", existing.value);
            self.spec.constraints.retain(|constraint| constraint != &previous);
            existing.value = value.to_string();
        } else {
            self.decisions.push(IntentDecision {
                key: key.to_string(),
                value: value.to_string(),
            });
        }
        self.spec.constraints.push(format!("{key}: {value}"));
        self.progress_reported = false;
        Ok(())
    }

    pub fn report_progress(&mut self, progress: IntentProgress) -> Result<()> {
        progress::validate(&self.spec, &progress)?;
        let blocked_reason = progress.blocked_reason.filter(|reason| !reason.trim().is_empty());
        self.completed_outcomes = progress.completed_outcomes;
        self.verification = progress.verification;
        self.blocked_reason = blocked_reason;
        self.progress_reported = true;
        if self.blocked_reason.is_some() {
            self.status = IntentStatus::Blocked;
            self.spec.status = "blocked".to_string();
        } else if progress.complete {
            self.status = IntentStatus::Ready;
            self.spec.status = "ready".to_string();
        } else {
            self.status = IntentStatus::Active;
            self.spec.status = "active".to_string();
        }
        Ok(())
    }

    pub fn finalize_success(&mut self) {
        if self.status == IntentStatus::Ready {
            self.status = IntentStatus::Completed;
            self.spec.status = "completed".to_string();
        }
    }

    pub fn complete_informational(&mut self) {
        self.status = IntentStatus::Completed;
        self.spec.status = "informational".to_string();
        self.blocked_reason = None;
        self.progress_reported = true;
    }

    pub fn complete_by_user(&mut self) {
        self.completed_outcomes = self.spec.outcomes.clone();
        self.status = IntentStatus::Completed;
        self.spec.status = "completed_by_user".to_string();
        self.blocked_reason = None;
        self.progress_reported = true;
    }

    pub fn pause(&mut self) {
        self.status = IntentStatus::Paused;
        self.spec.status = "paused".to_string();
    }

    pub fn abandon(&mut self) {
        self.status = IntentStatus::Abandoned;
        self.spec.status = "abandoned".to_string();
    }

    pub fn is_unfinished(&self) -> bool {
        matches!(
            self.status,
            IntentStatus::Active | IntentStatus::Blocked | IntentStatus::Ready | IntentStatus::Paused
        )
    }

    pub fn context_projection(&self, max_bytes: usize) -> Result<String> {
        let mut projection = self.spec.to_system_prompt_section();
        let pending = self
            .spec
            .outcomes
            .iter()
            .filter(|outcome| !self.completed_outcomes.contains(outcome))
            .collect::<Vec<_>>();
        if !pending.is_empty() {
            projection.push_str("- **Remaining Outcomes**:\n");
            for outcome in pending {
                projection.push_str(&format!("  * {outcome}\n"));
            }
        }
        if projection.len() > max_bytes {
            return Err(crate::error::AppError::Intent(format!(
                "Active IntentSpec exceeds the {max_bytes}-byte context limit"
            )));
        }
        Ok(projection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> IntentState {
        let mut spec = IntentSpec::from_prompt("fix auth");
        spec.outcomes.push("Regression test passes".to_string());
        spec.verification.push("cargo test auth".to_string());
        IntentState::new(spec, "/repo".to_string(), "session-1".to_string())
    }

    #[test]
    fn completion_requires_outcomes_and_verification() {
        let mut state = state();
        let error = state
            .report_progress(IntentProgress {
                completed_outcomes: Vec::new(),
                verification: Vec::new(),
                blocked_reason: None,
                complete: true,
            })
            .unwrap_err();
        assert!(error.to_string().contains("pending outcomes"));
        assert_eq!(state.status, IntentStatus::Active);
    }

    #[test]
    fn verified_progress_completes_and_follow_up_reopens() {
        let mut state = state();
        state
            .report_progress(IntentProgress {
                completed_outcomes: vec!["Regression test passes".to_string()],
                verification: vec![VerificationResult {
                    obligation: "cargo test auth".to_string(),
                    passed: true,
                }],
                blocked_reason: None,
                complete: true,
            })
            .unwrap();
        assert_eq!(state.status, IntentStatus::Ready);
        assert!(state.progress_reported);
        state.finalize_success();
        assert_eq!(state.status, IntentStatus::Completed);

        state.amend(&IntentSpec::from_prompt("also update docs"));
        assert_eq!(state.status, IntentStatus::Active);
        assert!(!state.progress_reported);
        assert!(
            !state
                .spec
                .outcomes
                .iter()
                .any(|outcome| outcome.starts_with("Follow-up: "))
        );
    }

    #[test]
    fn amendments_remove_legacy_generated_follow_up_outcomes() {
        let mut state = state();
        state.spec.outcomes.push("Follow-up: so?".to_string());

        state.amend(&IntentSpec::from_prompt("continue"));

        assert!(
            !state
                .spec
                .outcomes
                .iter()
                .any(|outcome| outcome.starts_with("Follow-up: "))
        );
        assert!(state.spec.outcomes.contains(&"Regression test passes".to_string()));
    }

    #[test]
    fn user_completion_resolves_pending_outcomes_without_fabricating_verification() {
        let mut state = state();
        state.complete_by_user();

        assert_eq!(state.status, IntentStatus::Completed);
        assert_eq!(state.completed_outcomes, state.spec.outcomes);
        assert!(state.verification.is_empty());
    }

    #[test]
    fn explicit_decisions_are_idempotent_and_can_be_revised() {
        let mut state = state();
        state.record_decision("auth.strategy", "sessions").unwrap();
        state.record_decision("auth.strategy", "sessions").unwrap();
        state.record_decision("auth.strategy", "jwt").unwrap();
        assert_eq!(state.decisions.len(), 1);
        assert_eq!(state.decisions[0].value, "jwt");
        assert!(!state.spec.constraints.contains(&"auth.strategy: sessions".to_string()));
    }
}
