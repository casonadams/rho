use super::parser::{GuardVerdict, parse_guard_output};
use super::prompt::GUARD_SYSTEM_PROMPT;
use rig::agent::ModelHandle;
use rig::completion::Prompt;
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
        let agent = rig::agent::AgentBuilder::from_model_handle(self.model.clone())
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
                reason: "Guard model evaluation timed out after 0s. Approval required.".to_string(),
            };
        }
        match tokio::time::timeout(self.timeout, agent.prompt(&eval_prompt)).await {
            Ok(Ok(response)) => parse_guard_output(&response),
            Ok(Err(err)) => GuardVerdict {
                safe: false,
                reason: format!("Guard model evaluation error: {err}"),
            },
            Err(_) => GuardVerdict {
                safe: false,
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
    async fn evaluate_safe_verdict() {
        let mock = MockCompletionModel::new([MockTurn::text(
            r#"{"safe": true, "reason": "Local directory creation"}"#,
        )]);
        let evaluator = GuardEvaluator::new(ModelHandle::new(mock));
        let verdict = evaluator.evaluate("mkdir -p src/foo").await;
        assert!(verdict.safe);
        assert_eq!(verdict.reason, "Local directory creation");
    }

    #[tokio::test]
    async fn evaluate_unsafe_verdict() {
        let mock = MockCompletionModel::new([MockTurn::text(
            r#"{"safe": false, "reason": "Git push modifies remote state"}"#,
        )]);
        let evaluator = GuardEvaluator::new(ModelHandle::new(mock));
        let verdict = evaluator.evaluate("git push origin main").await;
        assert!(!verdict.safe);
        assert_eq!(verdict.reason, "Git push modifies remote state");
    }

    #[tokio::test]
    async fn evaluate_handles_model_timeout_fail_closed() {
        let mock = MockCompletionModel::new([MockTurn::text(
            r#"{"safe": true, "reason": "Should not arrive before timeout"}"#,
        )]);
        let evaluator = GuardEvaluator::new(ModelHandle::new(mock)).with_timeout(Duration::from_millis(0));
        let verdict = evaluator.evaluate("cargo build").await;
        assert!(!verdict.safe);
        assert!(verdict.reason.contains("timed out"));
    }
}
