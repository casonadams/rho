use std::collections::BTreeMap;
use std::time::Duration;

use crate::decision::{DecisionModel, DecisionRequest};
use crate::engine::AgentEngine;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::text::truncate_chars;
use rho_harness_core::thinking::ThinkingLevel;

pub(crate) const AUTO_THINKING_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CLASSIFIER_PROMPT_CHARS: usize = 2000;
const EFFORT_QUESTION_KEY: &str = "effort";

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
    let (bounded, _) = truncate_chars(prompt.trim(), MAX_CLASSIFIER_PROMPT_CHARS);
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
        Ok(Err(error)) => {
            tracing::warn!(%error, "thinking classifier failed; using medium");
            ThinkingLevel::Medium
        }
        Err(_) => ThinkingLevel::Medium,
    }
}

impl AgentEngine {
    pub(super) async fn resolve_turn_thinking_level(
        &self,
        prompt: &str,
        presenter: &dyn Presenter,
    ) -> Option<ThinkingLevel> {
        if !ThinkingLevel::from_setting(self.config.thinking_level.as_deref()).is_auto() {
            return None;
        }
        let decision = match self.router.build_role_decision(&self.config, &["judge", "guard"]).await {
            Ok(Some(decision)) => decision,
            Ok(None) => return Some(ThinkingLevel::Medium),
            Err(error) => {
                crate::engine::report_unavailable_decision(presenter, "Automatic thinking level", &error);
                return Some(ThinkingLevel::Medium);
            }
        };
        Some(classify_thinking_effort(decision.as_ref(), prompt, AUTO_THINKING_TIMEOUT).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::types::test_util::MockDecisionModel;
    use crate::decision::{DecisionAnswer, DecisionResponse};
    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::provider::adapter::MockAdapter;
    use crate::test_support::{CapturingPresenter, TestDir};
    use rho_harness_core::config::Config;
    use rho_harness_core::error::AppError;
    use std::sync::Arc;
    use test_case::test_case;

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

    #[test_case("low", ThinkingLevel::Low; "low")]
    #[test_case("medium", ThinkingLevel::Medium; "medium")]
    #[test_case("high", ThinkingLevel::High; "high")]
    #[tokio::test]
    async fn classifies_low_medium_high_choices(choice: &str, expected: ThinkingLevel) {
        let model = MockDecisionModel::native(choice_response(choice));
        let level = classify_thinking_effort(&model, "implement feature", AUTO_THINKING_TIMEOUT).await;
        assert_eq!(level, expected);
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_falls_back_to_medium() {
        let model = SlowDecisionModel {
            delay: Duration::from_secs(10),
        };
        let level = classify_thinking_effort(&model, "refactor architecture", AUTO_THINKING_TIMEOUT).await;
        assert_eq!(level, ThinkingLevel::Medium);
    }

    fn auto_engine(dir: &TestDir, judge: Option<&str>) -> AgentEngine {
        let mut app_config = Config {
            auth_file: dir.join("auth.json"),
            thinking_level: Some("auto".to_string()),
            ..Config::default()
        };
        if let Some(judge) = judge {
            app_config.model_roles.insert("judge".to_string(), judge.to_string());
        }
        mock_engine(
            Arc::new(MockAdapter::text("ok")),
            MockEngineConfig::new(dir, app_config),
        )
    }

    #[tokio::test]
    async fn unconfigured_judge_falls_back_to_medium_without_a_notice() {
        let dir = TestDir::new();
        let engine = auto_engine(&dir, None);
        let presenter = CapturingPresenter::default();
        assert_eq!(
            engine.resolve_turn_thinking_level("refactor", &presenter).await,
            Some(ThinkingLevel::Medium)
        );
        assert!(presenter.notices.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unbuildable_judge_falls_back_to_medium_with_a_notice() {
        let dir = TestDir::new();
        let engine = auto_engine(&dir, Some("mystery/model"));
        let presenter = CapturingPresenter::default();
        assert_eq!(
            engine.resolve_turn_thinking_level("refactor", &presenter).await,
            Some(ThinkingLevel::Medium)
        );
        let notices = presenter.notices.lock().unwrap();
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(
            notices[0].starts_with("[Automatic thinking level unavailable"),
            "{notices:?}"
        );
        assert!(notices[0].contains("mystery"), "{notices:?}");
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
