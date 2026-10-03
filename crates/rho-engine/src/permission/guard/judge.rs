use crate::provider::systemone::{SystemOneAnswer, SystemOneClient, SystemOneQuestion, SystemOneRequest};
use std::collections::BTreeMap;
use std::time::Duration;

pub struct JudgeEvaluator {
    client: SystemOneClient,
    timeout: Duration,
}

impl JudgeEvaluator {
    pub fn new(endpoint_or_base: &str) -> Self {
        Self {
            client: SystemOneClient::new(endpoint_or_base),
            timeout: Duration::from_secs(5),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn build_safety_request(command: &str) -> SystemOneRequest {
        let mut questions = BTreeMap::new();
        questions.insert(
            "is_safe".to_string(),
            SystemOneQuestion::Noul {
                instructions: "Decide whether the shell command is safe to execute without user approval.".to_string(),
                criteria_true: "Read-only inspection, compiling code, running tests, local status checks, git log/status/diff.".to_string(),
                criteria_false: "Destructive file deletion (rm -rf), branch force-push, production deployments, dropping database tables, secret exfiltration.".to_string(),
            },
        );

        SystemOneRequest {
            state: format!("Shell command to evaluate:\n{command}"),
            questions,
        }
    }

    pub fn evaluate_response(response: &crate::provider::systemone::SystemOneResponse) -> Result<bool, String> {
        if let Some(SystemOneAnswer::Noul(noul)) = response.answers.get("is_safe") {
            Ok(noul.answer && noul.probability >= 0.90)
        } else {
            Err("Judge did not return a valid noul answer for 'is_safe'".to_string())
        }
    }

    pub async fn evaluate_bash_safety(&self, command: &str) -> Result<bool, String> {
        let req = Self::build_safety_request(command);
        let response = match tokio::time::timeout(self.timeout, self.client.decide(&req)).await {
            Ok(Ok(res)) => res,
            Ok(Err(e)) => return Err(format!("Judge evaluation failed: {e}")),
            Err(_) => return Err("Judge evaluation timed out".to_string()),
        };

        Self::evaluate_response(&response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_safety_request() {
        let req = JudgeEvaluator::build_safety_request("cargo test");
        assert!(req.state.contains("cargo test"));
        assert!(req.questions.contains_key("is_safe"));
    }

    #[test]
    fn test_evaluate_response_safe_and_unsafe() {
        use crate::provider::systemone::NoulAnswer;
        let mut answers = BTreeMap::new();
        answers.insert(
            "is_safe".to_string(),
            SystemOneAnswer::Noul(NoulAnswer {
                answer: true,
                probability: 0.98,
            }),
        );
        let resp = crate::provider::systemone::SystemOneResponse { answers };
        assert_eq!(JudgeEvaluator::evaluate_response(&resp), Ok(true));

        let mut low_prob = BTreeMap::new();
        low_prob.insert(
            "is_safe".to_string(),
            SystemOneAnswer::Noul(NoulAnswer {
                answer: true,
                probability: 0.60,
            }),
        );
        let resp_low = crate::provider::systemone::SystemOneResponse { answers: low_prob };
        assert_eq!(JudgeEvaluator::evaluate_response(&resp_low), Ok(false));
    }
}
