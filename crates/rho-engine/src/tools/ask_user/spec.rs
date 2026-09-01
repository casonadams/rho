use rho_core::args::AskUserArgs;
use rho_core::error::{AppError, Result};
use rho_core::presentation::questions::{InteractiveQuestionPort, UserAnswer, UserQuestion, UserQuestionOption};
use serde_json::Value;

pub(crate) fn extract_question_text(args: &AskUserArgs) -> String {
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

pub(crate) fn extract_str_from_map(map: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(ToString::to_string)
}

pub(crate) fn extract_vec_from_map(map: &serde_json::Map<String, Value>, key: &str) -> Option<Vec<Value>> {
    map.get(key).and_then(Value::as_array).cloned()
}

pub(crate) struct ParsedOption {
    pub(crate) label: String,
    pub(crate) description: Option<String>,
    pub(crate) value: String,
}

pub(crate) fn extract_parsed_option(option: &Value) -> ParsedOption {
    match option {
        Value::String(value) => ParsedOption {
            label: value.clone(),
            description: None,
            value: value.clone(),
        },
        Value::Object(object) if object.len() == 1 => {
            let (key, value) = object.iter().next().unwrap();
            let label = key.clone();
            let val_str = value.as_str().map_or_else(|| value.to_string(), ToString::to_string);
            ParsedOption {
                label,
                description: None,
                value: val_str,
            }
        }
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
                .or_else(|| object.get("hint"))
                .or_else(|| object.get("preview"))
                .or_else(|| object.get("help"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let value = object
                .get("value")
                .or_else(|| object.get("id"))
                .or_else(|| object.get("key"))
                .map(|val| val.as_str().map_or_else(|| val.to_string(), ToString::to_string))
                .unwrap_or_else(|| label.clone());
            ParsedOption {
                label,
                description,
                value,
            }
        }
        other => ParsedOption {
            label: other.to_string(),
            description: None,
            value: other.to_string(),
        },
    }
}

pub(crate) struct QuestionSpec<'a> {
    pub(crate) question: &'a str,
    pub(crate) header: Option<String>,
    pub(crate) options: Option<&'a [Value]>,
}

pub(crate) async fn ask_question(port: &dyn InteractiveQuestionPort, spec: QuestionSpec<'_>) -> Result<String> {
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

pub(crate) async fn prompt_question_value(
    port: &dyn InteractiveQuestionPort,
    value: &Value,
    index: usize,
) -> Result<String> {
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
