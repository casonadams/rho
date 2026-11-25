use std::collections::BTreeMap;
use std::time::Duration;

use crate::decision::{DecisionModel, DecisionRequest};
use crate::engine::AgentEngine;
use rho_harness_core::thinking::ThinkingLevel;

pub(crate) const AUTO_THINKING_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CLASSIFIER_PROMPT_CHARS: usize = 2000;
const EFFORT_QUESTION_KEY: &str = "effort";

fn truncate_prompt(prompt: &str, max_chars: usize) -> String {
    let trimmed = prompt.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    trimmed.chars().take(max_chars).collect()
}

fn effort_choice_criteria() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "low".to_string(),
            "Simple or direct queries, quick lookups, greetings, straightforward single-step edits, factual Q&A"
                .to_string(),
        ),
        (
            "medium".to_string(),
            "Standard multi-step coding tasks, moderate changes, routine debugging, code review".to_string(),
        ),
        (
            "high".to_string(),
            "Complex architecture, deep debugging, tricky concurrency or security bugs, multi-file refactors, subtle algorithmic reasoning"
                .to_string(),
        ),
    ])
}

fn map_effort_choice(choice: &str) -> ThinkingLevel {
    match choice.trim().to_ascii_lowercase().as_str() {
        "low" => ThinkingLevel::Low,
        "high" => ThinkingLevel::High,
        _ => ThinkingLevel::Medium,
    }
}

pub async fn classify_thinking_effort(decision: &dyn DecisionModel, prompt: &str, timeout: Duration) -> ThinkingLevel {
    let bounded = truncate_prompt(prompt, MAX_CLASSIFIER_PROMPT_CHARS);
    if bounded.is_empty() {
        return ThinkingLevel::Medium;
    }
    let request = DecisionRequest::new(bounded).with_choice(
        EFFORT_QUESTION_KEY,
        "Classify the reasoning effort required to answer this user prompt.",
        effort_choice_criteria(),
    );
    match tokio::time::timeout(timeout, decision.decide(request)).await {
        Ok(Ok(response)) => response
            .answers
            .get(EFFORT_QUESTION_KEY)
            .and_then(|answer| answer.as_choice())
            .map(map_effort_choice)
            .unwrap_or(ThinkingLevel::Medium),
        Ok(Err(_)) | Err(_) => ThinkingLevel::Medium,
    }
}

impl AgentEngine {
    pub(super) async fn resolve_turn_thinking_level(&self, prompt: &str) -> Option<ThinkingLevel> {
        if !ThinkingLevel::from_setting(self.config.thinking_level.as_deref()).is_auto() {
            return None;
        }
        let Ok(Some(decision)) = self.router.build_role_decision(&self.config, &["judge", "guard"]).await else {
            return Some(ThinkingLevel::Medium);
        };
        Some(classify_thinking_effort(decision.as_ref(), prompt, AUTO_THINKING_TIMEOUT).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::types::test_util::MockDecisionModel;
    use crate::decision::{DecisionAnswer, DecisionResponse};
    use rho_harness_core::error::AppError;

    fn choice_response(choice: &str) -> DecisionResponse {
        DecisionResponse {
            answers: BTreeMap::from([(
                EFFORT_QUESTION_KEY.to_string(),
                DecisionAnswer::Choice {
                    choice: choice.to_string(),
                    probabilities: BTreeMap::new(),
                    confidence: None,
                },
            )]),
            ..Default::default()
        }
    }

    #[derive(Debug)]
    struct SlowDecisionModel {
        delay: Duration,
    }

    #[async_trait::async_trait]
    impl DecisionModel for SlowDecisionModel {
        fn model_name(&self) -> &str {
            "slow-mock"
        }

        fn provider_name(&self) -> &str {
            "mock"
        }

        fn is_native_decision(&self) -> bool {
            true
        }

        async fn decide(&self, _request: DecisionRequest) -> Result<DecisionResponse, AppError> {
            tokio::time::sleep(self.delay).await;
            Ok(choice_response("high"))
        }
    }

    #[tokio::test]
    async fn classifies_low_medium_high_choices() {
        for (choice, expected) in [
            ("low", ThinkingLevel::Low),
            ("medium", ThinkingLevel::Medium),
            ("high", ThinkingLevel::High),
        ] {
            let model = MockDecisionModel::native(choice_response(choice));
            let level = classify_thinking_effort(&model, "implement feature", AUTO_THINKING_TIMEOUT).await;
            assert_eq!(level, expected);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_falls_back_to_medium() {
        let model = SlowDecisionModel {
            delay: Duration::from_secs(10),
        };
        let level = classify_thinking_effort(&model, "refactor architecture", AUTO_THINKING_TIMEOUT).await;
        assert_eq!(level, ThinkingLevel::Medium);
    }

    #[tokio::test]
    async fn error_and_invalid_choice_fall_back_to_medium() {
        let failing = MockDecisionModel::failing(AppError::Provider("judge offline".to_string()));
        assert_eq!(
            classify_thinking_effort(&failing, "debug crash", AUTO_THINKING_TIMEOUT).await,
            ThinkingLevel::Medium
        );

        let invalid = MockDecisionModel::native(choice_response("unknown-effort"));
        assert_eq!(
            classify_thinking_effort(&invalid, "debug crash", AUTO_THINKING_TIMEOUT).await,
            ThinkingLevel::Medium
        );
    }
}
