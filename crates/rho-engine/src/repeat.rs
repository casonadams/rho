use rig::agent::hook::{AgentHook, HookContext, ToolCall, ToolCallAction};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const REPEATED_CALL_MESSAGE: &str = "This identical tool call was blocked after three consecutive attempts. No operation was executed. Try a semantically different approach.";

#[derive(Clone, Default)]
struct RepeatedCallState {
    key: Option<String>,
    consecutive: usize,
}

#[derive(Clone)]
pub struct RepeatedCallHook {
    working_dir: PathBuf,
}

impl RepeatedCallHook {
    pub fn new(working_dir: impl AsRef<Path>) -> Self {
        Self {
            working_dir: working_dir.as_ref().to_path_buf(),
        }
    }
}

impl AgentHook for RepeatedCallHook {
    async fn on_tool_call(&self, ctx: &HookContext, event: ToolCall<'_>) -> ToolCallAction {
        let arguments =
            serde_json::from_str::<Value>(event.args).unwrap_or_else(|_| Value::String(event.args.trim().into()));
        let key = normalized_call_key(event.tool_name, &arguments, &self.working_dir);
        let consecutive = ctx.scratchpad().update::<RepeatedCallState, _>(|state| {
            if state.key.as_ref() == Some(&key) {
                state.consecutive += 1;
            } else {
                state.key = Some(key);
                state.consecutive = 1;
            }
            state.consecutive
        });
        if consecutive < 3 {
            return ToolCallAction::run();
        }
        ToolCallAction::skip(REPEATED_CALL_MESSAGE)
    }
}

pub fn normalized_call_key(tool_name: &str, arguments: &Value, working_dir: &Path) -> String {
    let mut normalized = arguments.clone();
    match tool_name {
        "bash" => normalize_bash(&mut normalized, working_dir),
        "web_search" => normalize_web_search(&mut normalized),
        _ => {}
    }
    serde_json::to_string(&(tool_name, normalized)).unwrap_or_else(|_| format!("{tool_name}:<invalid>"))
}

fn normalize_bash(arguments: &mut Value, working_dir: &Path) {
    let Some(values) = arguments.as_object_mut() else {
        return;
    };
    if let Some(command) = values.get_mut("command")
        && let Some(text) = command.as_str()
    {
        *command = Value::String(normalize_shell_whitespace(text));
    }
    values.insert(
        "working_directory".to_string(),
        Value::String(normalize_working_dir(working_dir)),
    );
}

fn normalize_web_search(arguments: &mut Value) {
    let Some(values) = arguments.as_object_mut() else {
        return;
    };
    if let Some(query) = values.get_mut("query")
        && let Some(text) = query.as_str()
    {
        *query = Value::String(
            text.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase(),
        );
    }
    let effective_limit = values.get("limit").and_then(Value::as_u64).unwrap_or(5).clamp(1, 20);
    values.insert("limit".to_string(), Value::from(effective_limit));
}

fn normalize_working_dir(working_dir: &Path) -> String {
    working_dir
        .canonicalize()
        .unwrap_or_else(|_| working_dir.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn normalize_shell_whitespace(command: &str) -> String {
    let mut output = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut pending_space = false;
    for character in command.trim().chars() {
        if escaped {
            if pending_space && !output.is_empty() {
                output.push(' ');
                pending_space = false;
            }
            output.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            if pending_space && !output.is_empty() {
                output.push(' ');
                pending_space = false;
            }
            output.push(character);
            escaped = true;
            continue;
        }
        if let Some(active) = quote {
            output.push(character);
            if character == active {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            if pending_space && !output.is_empty() {
                output.push(' ');
                pending_space = false;
            }
            quote = Some(character);
            output.push(character);
        } else if character.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space && !output.is_empty() {
                output.push(' ');
            }
            pending_space = false;
            output.push(character);
        }
    }
    output
}
