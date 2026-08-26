use crate::error::AppError;
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct AskUserArgs {
    /// The question to ask the user
    #[serde(default)]
    pub question: Option<String>,
    /// Optional choices/options for the user to select from
    #[serde(default)]
    pub options: Option<Vec<Value>>,
    /// Optional category header or short chip tag
    #[serde(default)]
    pub header: Option<String>,
    /// Optional batch array of questions
    #[serde(default)]
    pub questions: Option<Vec<Value>>,
    /// Catch-all for model-specific fields like prompt, message, text, query
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Clone, Default)]
pub struct AskUserTool;

impl AskUserTool {
    pub fn new() -> Self {
        Self
    }

    pub fn execute(&self, args: AskUserArgs) -> Result<ToolResult, AppError> {
        if let Some(ref questions) = args.questions
            && !questions.is_empty()
        {
            let mut results = Vec::new();
            for (idx, q_val) in questions.iter().enumerate() {
                let ans = prompt_question_value(q_val, idx + 1)?;
                results.push(ans);
            }
            return Ok(binding_clarification(results.join("\n")));
        }

        let question_text = extract_question_text(&args);
        let header = args.header.or_else(|| extract_str_from_map(&args.extra, "header"));
        let options = args
            .options
            .or_else(|| extract_vec_from_map(&args.extra, "options"))
            .or_else(|| extract_vec_from_map(&args.extra, "choices"));

        let ans = prompt_question_interactive(&question_text, header.as_deref(), options.as_deref())?;
        Ok(binding_clarification(ans))
    }
}

fn binding_clarification(answer: String) -> ToolResult {
    ToolResult::success(format!("Binding IntentSpec clarification:\n{answer}"))
}

fn extract_question_text(args: &AskUserArgs) -> String {
    if let Some(ref q) = args.question
        && !q.trim().is_empty()
    {
        return q.trim().to_string();
    }
    for key in ["prompt", "message", "text", "query", "title", "content", "input"] {
        if let Some(val) = extract_str_from_map(&args.extra, key)
            && !val.trim().is_empty()
        {
            return val.trim().to_string();
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

fn extract_parsed_option(opt: &Value) -> ParsedOption {
    match opt {
        Value::String(s) => ParsedOption {
            label: s.clone(),
            description: None,
            value: s.clone(),
        },
        Value::Object(obj) => {
            let label = obj
                .get("label")
                .or_else(|| obj.get("name"))
                .or_else(|| obj.get("text"))
                .or_else(|| obj.get("title"))
                .or_else(|| obj.get("value"))
                .and_then(Value::as_str)
                .unwrap_or("Option")
                .to_string();
            let desc = obj
                .get("description")
                .or_else(|| obj.get("desc"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .filter(|d| !d.trim().is_empty());
            let value = obj
                .get("value")
                .or_else(|| obj.get("label"))
                .and_then(Value::as_str)
                .unwrap_or(&label)
                .to_string();
            ParsedOption {
                label,
                description: desc,
                value,
            }
        }
        _ => ParsedOption {
            label: opt.to_string(),
            description: None,
            value: opt.to_string(),
        },
    }
}

fn prompt_question_value(q_val: &Value, index: usize) -> Result<String, AppError> {
    match q_val {
        Value::String(s) => {
            let ans = prompt_question_interactive(s, None, None)?;
            Ok(format!("{index}. {s}: {ans}"))
        }
        Value::Object(obj) => {
            let question = obj
                .get("question")
                .or_else(|| obj.get("prompt"))
                .or_else(|| obj.get("text"))
                .or_else(|| obj.get("title"))
                .and_then(Value::as_str)
                .unwrap_or("Question");
            let header = obj.get("header").and_then(Value::as_str);
            let options = obj
                .get("options")
                .or_else(|| obj.get("choices"))
                .and_then(Value::as_array);
            let ans = prompt_question_interactive(question, header, options.map(|v| v.as_slice()))?;
            Ok(format!("{index}. {question}: {ans}"))
        }
        _ => {
            let s = q_val.to_string();
            let ans = prompt_question_interactive(&s, None, None)?;
            Ok(format!("{index}. {s}: {ans}"))
        }
    }
}

fn prompt_question_interactive(
    question: &str,
    header: Option<&str>,
    options: Option<&[Value]>,
) -> Result<String, AppError> {
    if let Some(h) = header {
        println!("\n[{h}] {question}\n");
    } else {
        println!("\n{question}\n");
    }

    if let Some(opts) = options
        && !opts.is_empty()
    {
        let parsed: Vec<ParsedOption> = opts.iter().map(extract_parsed_option).collect();

        let has_descriptions = parsed.iter().any(|o| o.description.is_some());
        if has_descriptions {
            for opt in &parsed {
                if let Some(ref desc) = opt.description {
                    println!("• {}:\n  {desc}\n", opt.label);
                }
            }
        }

        let mut labels: Vec<String> = parsed.iter().map(|o| o.label.clone()).collect();
        let custom_choice = "Type a custom answer...".to_string();
        labels.push(custom_choice.clone());

        let ans = inquire::Select::new("Select an option:", labels).prompt();
        println!();
        match ans {
            Ok(choice) if choice == custom_choice => {
                let typed = inquire::Text::new("Your answer:").prompt().unwrap_or_default();
                println!();
                Ok(typed)
            }
            Ok(choice) => {
                let val = parsed
                    .iter()
                    .find(|o| o.label == choice)
                    .map(|o| o.value.clone())
                    .unwrap_or(choice);
                Ok(val)
            }
            Err(_) => Ok("User skipped / cancelled question.".to_string()),
        }
    } else {
        let ans = inquire::Text::new("Your answer:").prompt();
        println!();
        match ans {
            Ok(text) => Ok(text),
            Err(_) => Ok("User skipped / cancelled question.".to_string()),
        }
    }
}

impl Tool for AskUserTool {
    const NAME: &'static str = "ask_user";
    type Args = AskUserArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "After inspecting available context, ask one consolidated set of questions for unresolved decisions that only the user can make. Answers are binding additions to the active IntentSpec.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<AskUserArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let res = tokio::task::spawn_blocking(move || {
            let _ = std::io::stdout().flush();
            AskUserTool.execute(args)
        })
        .await
        .map_err(|e| ToolExecutionError::other(format!("ask_user prompt task failed: {e}")))?;

        into_rig_result(res)
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
        "After inspecting available context, ask one consolidated set of questions for unresolved decisions that only the user can make. Answers are binding additions to the active IntentSpec.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<AskUserArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let res = tokio::task::spawn_blocking(move || {
            let _ = std::io::stdout().flush();
            AskUserTool.execute(args)
        })
        .await
        .map_err(|e| ToolExecutionError::other(format!("ask_user prompt task failed: {e}")))?;

        into_rig_result(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clarification_result_marks_the_answer_as_binding() {
        let result = binding_clarification("Use sessions".to_string());

        assert!(result.content.contains("Binding IntentSpec clarification"));
        assert!(result.content.contains("Use sessions"));
    }

    #[test]
    fn test_ask_user_parses_arbitrary_structures() {
        let json1 = serde_json::json!({
            "prompt": "What should I do?",
            "choices": ["Option 1", "Option 2"]
        });
        let parsed1 = serde_json::from_value::<AskUserArgs>(json1).unwrap();
        assert_eq!(extract_question_text(&parsed1), "What should I do?");

        let json2 = serde_json::json!({
            "questions": [
                { "title": "Framework?", "options": [{ "name": "React" }] }
            ]
        });
        let parsed2 = serde_json::from_value::<AskUserArgs>(json2).unwrap();
        assert!(parsed2.questions.is_some());

        let json3 = serde_json::json!({});
        let parsed3 = serde_json::from_value::<AskUserArgs>(json3).unwrap();
        assert_eq!(extract_question_text(&parsed3), "Please provide your input:");
    }
}
