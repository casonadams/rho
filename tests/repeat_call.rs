// Repeat-call protection hook behavior against the real bash tool; drives
// the hook through rig's agent loop with real host-workspace tools.

#[cfg(test)]
mod tests {
    use rho::engine::repeat::{REPEATED_CALL_MESSAGE, RepeatedCallHook, normalized_call_key};
    use rho::tools::BashTool;
    use rig::agent::AgentBuilder;
    use rig::test_utils::{MockCompletionModel, MockTurn};
    use serde_json::{Value, json};
    use std::path::Path;

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
            key("web_search", json!({"query":" Rig   Memory ", "limit":null})),
            key("web_search", json!({"query":"rig memory", "limit":5}))
        );
        assert_ne!(
            key("web_search", json!({"query":"rig memory", "limit":5})),
            key("web_search", json!({"query":"rig memory hook", "limit":5}))
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
        let agent = AgentBuilder::new(model.clone())
            .tool(BashTool::new(&dir))
            .add_hook(RepeatedCallHook::new(&dir))
            .build();
        let response = agent.runner("repeat").max_turns(5).run().await.unwrap();

        assert_eq!(response.output, "changed approach");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "xx");
        let third_request = format!("{:?}", model.requests()[3].chat_history);
        assert!(third_request.contains("blocked after three consecutive attempts"));
    }

    #[tokio::test]
    async fn consecutive_calls_count_toward_the_same_consecutive_threshold() {
        let dir = std::env::temp_dir().join(format!("repeat_consecutive_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.txt");
        let arguments = json!({"path":path,"content":"same"});
        let model = MockCompletionModel::new([
            MockTurn::tool_call("a", "write", arguments.clone()),
            MockTurn::tool_call("b", "write", arguments.clone()),
            MockTurn::tool_call("c", "write", arguments),
            MockTurn::text("done"),
        ]);
        let agent = AgentBuilder::new(model.clone())
            .tool(rho::tools::WriteTool::new(&dir))
            .add_hook(RepeatedCallHook::new(&dir))
            .build();
        agent.runner("repeat").max_turns(5).run().await.unwrap();

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
            .tool(rho::tools::ReadTool::new(&dir))
            .add_hook(RepeatedCallHook::new(&dir))
            .build();
        agent.runner("read").max_turns(6).run().await.unwrap();

        let final_history = format!("{:?}", model.requests().last().unwrap().chat_history);
        assert!(!final_history.contains(REPEATED_CALL_MESSAGE));
    }
}
