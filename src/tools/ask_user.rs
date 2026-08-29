use std::sync::Arc;

use async_trait::async_trait;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppError;
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct AskUserArgs {
    /// The complete question to ask the user. Should be clear, specific, and end with a question mark.
    #[serde(default)]
    pub question: Option<String>,
    /// The available choices for this question (2-4 options recommended). Each option can be a string or an object with label and description.
    #[serde(default)]
    pub options: Option<Vec<Value>>,
    /// Very short chip/tag shown next to the question (1-3 words, e.g. "Library", "Approach", "Auth").
    #[serde(default)]
    pub header: Option<String>,
    /// List of multiple questions to ask in a single prompt sequence.
    #[serde(default)]
    pub questions: Option<Vec<Value>>,
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserQuestionOption {
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserQuestion {
    pub question: String,
    pub header: Option<String>,
    pub options: Vec<UserQuestionOption>,
    pub allow_custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserAnswer {
    Selected(usize),
    Custom(String),
    Cancelled,
}

#[async_trait]
pub trait InteractiveQuestionPort: Send + Sync {
    async fn ask(&self, question: UserQuestion) -> Result<UserAnswer, AppError>;
}

#[derive(Clone)]
pub struct QuestionPort(Arc<dyn InteractiveQuestionPort>);

impl QuestionPort {
    pub fn new(port: impl InteractiveQuestionPort + 'static) -> Self {
        Self(Arc::new(port))
    }

    pub async fn ask(&self, question: UserQuestion) -> Result<UserAnswer, AppError> {
        self.0.ask(question).await
    }
}

#[async_trait]
impl InteractiveQuestionPort for QuestionPort {
    async fn ask(&self, question: UserQuestion) -> Result<UserAnswer, AppError> {
        self.0.ask(question).await
    }
}

#[derive(Clone, Default)]
pub struct AskUserTool;

impl AskUserTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(&self, port: &dyn InteractiveQuestionPort, args: AskUserArgs) -> Result<ToolResult, AppError> {
        if let Some(questions) = args.questions.as_deref().filter(|questions| !questions.is_empty()) {
            let mut results = Vec::with_capacity(questions.len());
            for (index, question) in questions.iter().enumerate() {
                results.push(prompt_question_value(port, question, index + 1).await?);
            }
            return Ok(ToolResult::success(results.join("\n")));
        }

        let question = extract_question_text(&args);
        let header = args.header.or_else(|| extract_str_from_map(&args.extra, "header"));
        let options = args
            .options
            .or_else(|| extract_vec_from_map(&args.extra, "options"))
            .or_else(|| extract_vec_from_map(&args.extra, "choices"));
        let answer = ask_question(
            port,
            QuestionSpec {
                question: &question,
                header,
                options: options.as_deref(),
            },
        )
        .await?;
        Ok(ToolResult::success(answer))
    }
}

fn extract_question_text(args: &AskUserArgs) -> String {
    if let Some(question) = args.question.as_deref().filter(|question| !question.trim().is_empty()) {
        return question.trim().to_string();
    }
    for key in ["prompt", "message", "text", "query", "title", "content", "input"] {
        if let Some(value) = extract_str_from_map(&args.extra, key).filter(|value| !value.trim().is_empty()) {
            return value.trim().to_string();
        }
    }
    "Please provide your input:".to_string()
}

fn extract_str_from_map(map: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(ToString::to_string)
}

fn extract_vec_from_map(map: &serde_json::Map<String, Value>, key: &str) -> Option<Vec<Value>> {
    map.get(key).and_then(Value::as_array).cloned()
}

struct ParsedOption {
    label: String,
    description: Option<String>,
    value: String,
}

fn extract_parsed_option(option: &Value) -> ParsedOption {
    match option {
        Value::String(value) => ParsedOption {
            label: value.clone(),
            description: None,
            value: value.clone(),
        },
        Value::Object(object) => {
            let label = object
                .get("label")
                .or_else(|| object.get("name"))
                .or_else(|| object.get("text"))
                .or_else(|| object.get("title"))
                .or_else(|| object.get("value"))
                .and_then(Value::as_str)
                .unwrap_or("Option")
                .to_string();
            let description = object
                .get("description")
                .or_else(|| object.get("desc"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .filter(|description| !description.trim().is_empty());
            let value = object
                .get("value")
                .or_else(|| object.get("label"))
                .and_then(Value::as_str)
                .unwrap_or(&label)
                .to_string();
            ParsedOption {
                label,
                description,
                value,
            }
        }
        _ => ParsedOption {
            label: option.to_string(),
            description: None,
            value: option.to_string(),
        },
    }
}

async fn prompt_question_value(
    port: &dyn InteractiveQuestionPort,
    value: &Value,
    index: usize,
) -> Result<String, AppError> {
    let (question, header, options) = match value {
        Value::String(question) => (question.as_str(), None, None),
        Value::Object(object) => {
            let question = object
                .get("question")
                .or_else(|| object.get("prompt"))
                .or_else(|| object.get("text"))
                .or_else(|| object.get("title"))
                .and_then(Value::as_str)
                .unwrap_or("Question");
            let header = object.get("header").and_then(Value::as_str).map(ToString::to_string);
            let options = object
                .get("options")
                .or_else(|| object.get("choices"))
                .and_then(Value::as_array)
                .map(Vec::as_slice);
            (question, header, options)
        }
        _ => ("Question", None, None),
    };
    let answer = ask_question(
        port,
        QuestionSpec {
            question,
            header,
            options,
        },
    )
    .await?;
    Ok(format!("{index}. {question}: {answer}"))
}

struct QuestionSpec<'a> {
    question: &'a str,
    header: Option<String>,
    options: Option<&'a [Value]>,
}

async fn ask_question(port: &dyn InteractiveQuestionPort, spec: QuestionSpec<'_>) -> Result<String, AppError> {
    let parsed = spec
        .options
        .unwrap_or_default()
        .iter()
        .map(extract_parsed_option)
        .collect::<Vec<_>>();
    let answer = port
        .ask(UserQuestion {
            question: spec.question.to_string(),
            header: spec.header,
            options: parsed
                .iter()
                .map(|option| UserQuestionOption {
                    label: option.label.clone(),
                    description: option.description.clone(),
                })
                .collect(),
            allow_custom: true,
        })
        .await?;
    match answer {
        UserAnswer::Selected(index) => parsed
            .get(index)
            .map(|option| option.value.clone())
            .ok_or_else(|| AppError::Cancelled("Question returned an invalid selection".to_string())),
        UserAnswer::Custom(answer) => Ok(answer),
        UserAnswer::Cancelled => Err(AppError::Cancelled("Question cancelled by user".to_string())),
    }
}

impl Tool for AskUserTool {
    const NAME: &'static str = "ask_user";
    type Args = AskUserArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Ask the user one or more structured questions to clarify ambiguous requirements, confirm architectural choices, or gather user preferences.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<AskUserArgs>()
    }

    async fn call(&self, context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let port = context
            .get::<QuestionPort>()
            .cloned()
            .ok_or_else(|| ToolExecutionError::other("interactive question port is unavailable"))?;
        into_rig_result(self.execute(&port, args).await)
    }
}

#[derive(Clone, Default)]
pub struct AskUserQuestionTool(pub AskUserTool);

impl Tool for AskUserQuestionTool {
    const NAME: &'static str = "ask_user_question";
    type Args = AskUserArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        self.0.description()
    }

    fn parameters(&self) -> serde_json::Value {
        self.0.parameters()
    }

    async fn call(&self, context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let port = context
            .get::<QuestionPort>()
            .cloned()
            .ok_or_else(|| ToolExecutionError::other("interactive question port is unavailable"))?;
        into_rig_result(self.0.execute(&port, args).await)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

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
}
