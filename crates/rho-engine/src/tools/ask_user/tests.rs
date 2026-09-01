use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use super::spec::extract_question_text;
use super::*;

struct FakePort {
    answers: Mutex<VecDeque<UserAnswer>>,
    questions: Arc<Mutex<Vec<UserQuestion>>>,
}

#[async_trait]
impl InteractiveQuestionPort for FakePort {
    async fn ask(&self, question: UserQuestion) -> Result<UserAnswer, AppError> {
        self.questions.lock().unwrap().push(question);
        Ok(self.answers.lock().unwrap().pop_front().unwrap())
    }
}

fn port(answers: impl IntoIterator<Item = UserAnswer>) -> QuestionPort {
    QuestionPort::new(FakePort {
        answers: Mutex::new(answers.into_iter().collect()),
        questions: Arc::new(Mutex::new(Vec::new())),
    })
}

#[tokio::test]
async fn single_key_object_option_uses_key_as_label_and_value() {
    let port = QuestionPort::new(FakePort {
        answers: Mutex::new(VecDeque::from([UserAnswer::Selected(0)])),
        questions: Arc::new(Mutex::new(Vec::new())),
    });
    let result = AskUserTool
        .execute(
            &port,
            serde_json::from_value(serde_json::json!({
                "question": "Color?",
                "options": [{"blue": "Blue"}, {"red": "Red"}]
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.content, "Blue");
}

#[tokio::test]
async fn option_selection_returns_value_and_preserves_description() {
    let questions = Arc::new(Mutex::new(Vec::new()));
    let port = QuestionPort::new(FakePort {
        answers: Mutex::new(VecDeque::from([UserAnswer::Selected(0)])),
        questions: Arc::clone(&questions),
    });
    let result = AskUserTool
        .execute(
            &port,
            serde_json::from_value(serde_json::json!({
                "question": "Framework?",
                "options": [{"label": "React", "value": "react", "description": "Web UI"}]
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.content, "react");
    assert_eq!(
        questions.lock().unwrap()[0].options,
        [UserQuestionOption {
            label: "React".to_string(),
            description: Some("Web UI".to_string()),
        }]
    );
}

#[tokio::test]
async fn custom_text_and_multiple_questions_are_supported() {
    let port = port([UserAnswer::Custom("first answer".to_string()), UserAnswer::Selected(1)]);
    let result = AskUserTool
        .execute(
            &port,
            serde_json::from_value(serde_json::json!({
                "questions": [
                    {"question": "First?"},
                    {"question": "Second?", "options": ["A", "B"]}
                ]
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.content, "1. First?: first answer\n2. Second?: B");
}

#[tokio::test]
async fn cancellation_is_explicit() {
    let error = AskUserTool
        .execute(
            &port([UserAnswer::Cancelled]),
            AskUserArgs {
                question: Some("Continue?".to_string()),
                ..AskUserArgs::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Cancelled(_)));
}

#[test]
fn schemas_and_flexible_question_fields_are_preserved() {
    let parsed = serde_json::from_value::<AskUserArgs>(serde_json::json!({
        "prompt": "What should I do?",
        "choices": ["Option 1", "Option 2"]
    }))
    .unwrap();
    assert_eq!(extract_question_text(&parsed), "What should I do?");
    assert!(AskUserTool.parameters().get("properties").is_some());
    assert_eq!(AskUserTool.parameters(), AskUserQuestionTool::default().parameters());
}
