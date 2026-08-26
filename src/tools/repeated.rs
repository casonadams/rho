use crate::tools::approval::{ApprovalEventSink, ToolEvent};
use crate::tools::policy::ToolExecutionPolicy;
use rig::agent::hook::{AgentHook, HookContext, ToolCall, ToolCallAction};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const REPEATED_CALL_MESSAGE: &str = "This identical tool call was blocked after three consecutive attempts. No operation was executed. Try a semantically different approach.";

#[derive(Clone, Default)]
struct RepeatedCallState {
    key: Option<String>,
    consecutive: usize,
}

#[derive(Clone)]
pub struct RepeatedCallHook {
    working_dir: PathBuf,
    sink: Option<Arc<dyn ApprovalEventSink>>,
}

impl RepeatedCallHook {
    pub fn new(working_dir: impl AsRef<Path>) -> Self {
        Self {
            working_dir: working_dir.as_ref().to_path_buf(),
            sink: None,
        }
    }

    pub fn with_sink(mut self, sink: Arc<dyn ApprovalEventSink>) -> Self {
        self.sink = Some(sink);
        self
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
        if let Some(sink) = &self.sink {
            sink.emit(ToolEvent::CallClassified {
                internal_call_id: event.internal_call_id.to_string(),
                tool_name: event.tool_name.to_string(),
                arguments: arguments.clone(),
                class: ToolExecutionPolicy::classify(event.tool_name, &arguments),
            });
        }
        ToolCallAction::skip(REPEATED_CALL_MESSAGE)
    }
}

fn normalized_call_key(tool_name: &str, arguments: &Value, working_dir: &Path) -> String {
    let mut normalized =
        ToolExecutionPolicy::canonical_arguments(tool_name, arguments).unwrap_or_else(|| arguments.clone());
    match tool_name {
        "bash" => normalize_bash(&mut normalized, working_dir),
        "websearch" => normalize_web_search(&mut normalized),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{
        ApprovalCapability, ApprovalDecision, ApprovalEventSink, ApprovalHook, ApprovalRequest, BashTool,
        approval_context,
    };
    use async_trait::async_trait;
    use rig::agent::AgentBuilder;
    use rig::test_utils::{MockCompletionModel, MockTurn};
    use serde_json::json;

    struct Approve;

    #[async_trait]
    impl ApprovalEventSink for Approve {
        async fn request_approval(&self, _request: ApprovalRequest) -> ApprovalDecision {
            ApprovalDecision::Approved
        }
    }

    fn key(name: &str, arguments: Value) -> String {
        normalized_call_key(name, &arguments, Path::new("."))
    }

    #[test]
    fn normalization_preserves_meaningful_differences() {
        assert_eq!(
            key("bash", json!({"command":"  cargo   test  ", "timeout":30})),
            key("bash", json!({"command":"cargo test", "timeout":30}))
        );
        assert_ne!(
            key("bash", json!({"command":"printf 'a  b'", "timeout":30})),
            key("bash", json!({"command":"printf 'a b'", "timeout":30}))
        );
        assert_ne!(
            key("bash", json!({"command":"cargo test", "timeout":30})),
            key("bash", json!({"command":"cargo test", "timeout":31}))
        );
        assert_eq!(
            key("websearch", json!({"query":" Rig   Memory ", "limit":null})),
            key("websearch", json!({"query":"rig memory", "limit":5}))
        );
        assert_ne!(
            key("websearch", json!({"query":"rig memory", "limit":5})),
            key("websearch", json!({"query":"rig memory hook", "limit":5}))
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn third_whitespace_normalized_mutation_is_blocked_without_side_effect() {
        let dir = std::env::temp_dir().join(format!("repeat_hook_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("marker");
        let commands = [
            format!("printf x >> {}", marker.display()),
            format!("  printf   x   >>   {}  ", marker.display()),
            format!("printf x >> {}", marker.display()),
        ];
        let model = MockCompletionModel::new([
            MockTurn::tool_call("a", "bash", json!({"command":commands[0]})),
            MockTurn::tool_call("b", "bash", json!({"command":commands[1]})),
            MockTurn::tool_call("c", "bash", json!({"command":commands[2]})),
            MockTurn::text("changed approach"),
        ]);
        let capability = ApprovalCapability::new(true, Arc::new(Approve));
        let agent = AgentBuilder::new(model.clone())
            .tool(BashTool::new(&dir))
            .add_hook(RepeatedCallHook::new(&dir))
            .build();
        let response = agent
            .runner("repeat")
            .tool_context(approval_context(capability))
            .tool_concurrency(1)
            .max_turns(5)
            .run()
            .await
            .unwrap();

        assert_eq!(response.output, "changed approach");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "xx");
        let third_request = format!("{:?}", model.requests()[3].chat_history);
        assert!(third_request.contains("blocked after three consecutive attempts"));
    }

    struct Deny;

    #[async_trait]
    impl ApprovalEventSink for Deny {
        async fn request_approval(&self, _request: ApprovalRequest) -> ApprovalDecision {
            ApprovalDecision::Denied {
                reason: "offline test".to_string(),
            }
        }
    }

    #[tokio::test]
    async fn denied_calls_count_toward_the_same_consecutive_threshold() {
        let dir = std::env::temp_dir().join(format!("repeat_denied_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".git/config");
        let arguments = json!({"path":path,"content":"forbidden"});
        let model = MockCompletionModel::new([
            MockTurn::tool_call("a", "write", arguments.clone()),
            MockTurn::tool_call("b", "write", arguments.clone()),
            MockTurn::tool_call("c", "write", arguments),
            MockTurn::text("done"),
        ]);
        let capability = ApprovalCapability::new(false, Arc::new(Deny));
        let agent = AgentBuilder::new(model.clone())
            .tool(crate::tools::WriteTool::new(&dir))
            .add_hook(RepeatedCallHook::new(&dir))
            .add_hook(ApprovalHook::new(capability.clone()))
            .build();
        agent
            .runner("repeat denied")
            .tool_context(approval_context(capability))
            .tool_concurrency(1)
            .max_turns(5)
            .run()
            .await
            .unwrap();

        assert!(!path.exists());
        let history = format!("{:?}", model.requests()[3].chat_history);
        assert!(history.contains("blocked after three consecutive attempts"));
    }

    #[tokio::test]
    async fn changed_and_interleaved_calls_reset_while_failures_still_count() {
        let dir = std::env::temp_dir().join(format!("repeat_reset_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let missing = dir.join("missing");
        let other = dir.join("other");
        let model = MockCompletionModel::new([
            MockTurn::tool_call("a", "read", json!({"path": missing})),
            MockTurn::tool_call("b", "read", json!({"path": missing})),
            MockTurn::tool_call("c", "read", json!({"path": other})),
            MockTurn::tool_call("d", "read", json!({"path": missing})),
            MockTurn::text("done"),
        ]);
        let agent = AgentBuilder::new(model.clone())
            .tool(crate::tools::ReadTool::new(&dir))
            .add_hook(RepeatedCallHook::new(&dir))
            .build();
        agent.runner("read").max_turns(6).run().await.unwrap();

        let final_history = format!("{:?}", model.requests().last().unwrap().chat_history);
        assert!(!final_history.contains(REPEATED_CALL_MESSAGE));
    }
}
