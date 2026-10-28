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

        let eval_prompt = format!("<command_to_evaluate>\n{command}\n</command_to_evaluate>");
        if self.timeout.is_zero() {
            return GuardVerdict {
                safe: false,
                action: None,
                reason: "Guard model evaluation timed out after 0s. Approval required.".to_string(),
            };
        }

        let req = crate::provider::adapter::ModelCompletionRequest {
            preamble: Some(GUARD_SYSTEM_PROMPT.to_string()),
            prompt: eval_prompt,
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: Some(256),
            temperature: Some(0.0),
        };

        match tokio::time::timeout(self.timeout, self.model.complete(req)).await {
            Ok(Ok(response)) => parse_guard_output(&response.content),
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
    use crate::provider::adapter::MockAdapter;
    use std::sync::Arc;

    #[tokio::test]
    async fn evaluate_critical_danger_intercepts_without_calling_model() {
        let mock = Arc::new(MockAdapter::default());
        let evaluator = GuardEvaluator::new(mock);
        let verdict = evaluator.evaluate("rm -rf /").await;
        assert!(!verdict.safe);
        assert!(verdict.reason.contains("filesystem wipe"));
        assert!(verdict.action.is_some());
    }

    #[tokio::test]
    async fn evaluate_safe_verdict() {
        let mock = Arc::new(MockAdapter::text(
            r#"{"safe": true, "reason": "Local directory creation"}"#,
        ));
        let evaluator = GuardEvaluator::new(mock);
        let verdict = evaluator.evaluate("mkdir -p src/foo").await;
        assert!(verdict.safe);
        assert_eq!(verdict.reason, "Local directory creation");
    }

    #[tokio::test]
    async fn evaluate_unsafe_verdict() {
        let mock = Arc::new(MockAdapter::text(
            r#"{"safe": false, "reason": "Git push modifies remote state"}"#,
        ));
        let evaluator = GuardEvaluator::new(mock);
        let verdict = evaluator.evaluate("git push origin main").await;
        assert!(!verdict.safe);
        assert_eq!(verdict.reason, "Git push modifies remote state");
    }

    #[tokio::test]
    async fn evaluate_handles_model_timeout_fail_closed() {
        let mock = Arc::new(MockAdapter::text(
            r#"{"safe": true, "reason": "Should not arrive before timeout"}"#,
        ));
        let evaluator = GuardEvaluator::new(mock).with_timeout(Duration::from_millis(0));
        let verdict = evaluator.evaluate("cargo build").await;
        assert!(!verdict.safe);
        assert!(verdict.reason.contains("timed out"));
    }
}
