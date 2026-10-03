use super::danger::check_critical_danger;
use super::parser::{GuardVerdict, parse_guard_output};
use super::prompt::GUARD_SYSTEM_PROMPT;
use crate::engine::compactor::llm::ModelHandle;
use std::time::Duration;

#[derive(Clone)]
pub struct GuardEvaluator {
    model: ModelHandle,
    timeout: Duration,
}

impl GuardEvaluator {
    pub fn new(model: ModelHandle) -> Self {
        Self {
            model,
            timeout: Duration::from_secs(10),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn evaluate(&self, command: &str) -> GuardVerdict {
        if let Some(verdict) = check_critical_danger(command) {
            return verdict;
        }

        let agent = rig::agent::AgentBuilder::new(self.model.clone())
            .preamble(GUARD_SYSTEM_PROMPT)
            .default_max_turns(1)
            .max_tokens(256)
            .temperature(0.0)
            .record_content_telemetry(false)
            .build();

        let eval_prompt = format!("<command_to_evaluate>\n{command}\n</command_to_evaluate>");
        if self.timeout.is_zero() {
            return GuardVerdict {
                safe: false,
                action: None,
                reason: "Guard model evaluation timed out after 0s. Approval required.".to_string(),
            };
        }
        match tokio::time::timeout(self.timeout, agent.prompt(&eval_prompt).run()).await {
            Ok(Ok(response)) => parse_guard_output(&response.output),
            Ok(Err(err)) => GuardVerdict {
                safe: false,
                action: None,
                reason: format!("Guard model evaluation error: {err}"),
            },
            Err(_) => GuardVerdict {
                safe: false,
                action: None,
                reason: format!("Guard model evaluation timed out after {:?}", self.timeout),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::test_utils::{MockCompletionModel, MockTurn};

    #[tokio::test]
    async fn evaluate_critical_danger_intercepts_without_calling_model() {
        // Model with zero turns would fail if called
        let mock = MockCompletionModel::from_turns([]).erase();
        let evaluator = GuardEvaluator::new(mock);
        let verdict = evaluator.evaluate("rm -rf /").await;
        assert!(!verdict.safe);
        assert!(verdict.reason.contains("filesystem wipe"));
        assert!(verdict.action.is_some());
    }

    #[tokio::test]
    async fn evaluate_safe_verdict() {
        let mock = MockCompletionModel::from_turns([MockTurn::text(
            r#"{"safe": true, "reason": "Local directory creation"}"#,
        )])
        .erase();
        let evaluator = GuardEvaluator::new(mock);
        let verdict = evaluator.evaluate("mkdir -p src/foo").await;
        assert!(verdict.safe);
        assert_eq!(verdict.reason, "Local directory creation");
    }

    #[tokio::test]
    async fn evaluate_unsafe_verdict() {
        let mock = MockCompletionModel::from_turns([MockTurn::text(
            r#"{"safe": false, "reason": "Git push modifies remote state"}"#,
        )])
        .erase();
        let evaluator = GuardEvaluator::new(mock);
        let verdict = evaluator.evaluate("git push origin main").await;
        assert!(!verdict.safe);
        assert_eq!(verdict.reason, "Git push modifies remote state");
    }

    #[tokio::test]
    async fn evaluate_handles_model_timeout_fail_closed() {
        let mock = MockCompletionModel::from_turns([MockTurn::text(
            r#"{"safe": true, "reason": "Should not arrive before timeout"}"#,
        )])
        .erase();
        let evaluator = GuardEvaluator::new(mock).with_timeout(Duration::from_millis(0));
        let verdict = evaluator.evaluate("cargo build").await;
        assert!(!verdict.safe);
        assert!(verdict.reason.contains("timed out"));
    }
}
