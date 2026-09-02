use super::*;
use async_trait::async_trait;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::{InteractionPrompt, InteractionResponse};
use rig::agent::AgentBuilder;
use rig::test_utils::{MockCompletionModel, MockTurn};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

fn noop_presenter() -> Arc<dyn Presenter> {
    crate::engine::eval::presenter::presenter()
}

/// Presenter with an interactive UI that always answers with one scripted
/// choice, for exercising the ui/prompt round trip.
struct ScriptedPresenter {
    response: InteractionResponse,
}

#[async_trait]
impl Presenter for ScriptedPresenter {
    fn write_output(&self, _text: &str) {}
    fn print_welcome(&self, _display: &rho_harness_core::presentation::WelcomeDisplay) {}
    fn print_session_status(&self, _display: &rho_harness_core::presentation::SessionStatus) {}
    fn print_notice(&self, _text: &str) {}
    fn print_user_block(&self, _input: &str) {}
    fn print_token(&self, _token: &str) {}
    fn print_thinking_token(&self, _token: &str) {}
    fn finish_tool_line(&self, _line: rho_harness_core::presentation::ToolLine) {}
    fn flush(&self) {}
    fn has_interactive_ui(&self) -> bool {
        true
    }
    fn start_spinner(&self, _message: &str) -> rho_harness_core::presentation::ActivityToken {
        rho_harness_core::presentation::ActivityToken::default()
    }
    fn start_tool_spinner(&self, _name: &str, _arguments: &Value) -> rho_harness_core::presentation::ActivityToken {
        rho_harness_core::presentation::ActivityToken::default()
    }
    fn start_tool_run(&self, _name: &str, _arguments: &Value) {}
    fn stream_port(&self) -> rho_harness_core::presentation::ToolStreamPort {
        rho_harness_core::presentation::ToolStreamPort::default()
    }
    async fn request_interaction(&self, _prompt: InteractionPrompt) -> Option<InteractionResponse> {
        Some(self.response.clone())
    }
}

fn prompt_payload(ui_prompt: bool) -> Vec<u8> {
    json!({
        "event": "pre_tool_call",
        "tool": "bash",
        "arguments": {"command": "cargo test"},
        "capabilities": {"ui_prompt": ui_prompt}
    })
    .to_string()
    .into_bytes()
}

fn script_plugin(dir: &std::path::Path, name: &str, body: &str) -> PluginConfig {
    use std::os::unix::fs::PermissionsExt;

    let script_path = dir.join(name);
    fs::write(&script_path, format!("#!/bin/sh\n{body}")).unwrap();
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    let config = PluginConfig {
        command: Some(script_path.to_string_lossy().into_owned()),
        ..PluginConfig::default()
    };
    config
}

#[tokio::test]
async fn empty_plugin_hook_runs_cleanly() {
    let plugins = BTreeMap::new();
    let hook = PluginHook::new(&plugins, ".", noop_presenter());
    assert!(hook.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn plugin_hook_allows_when_exit_zero() {
    let dir = tempdir().unwrap();
    let config = script_plugin(dir.path(), "allow_hook.sh", "exit 0\n");
    let mut plugins = BTreeMap::new();
    plugins.insert("allow_plugin".to_string(), config.clone());

    let hook = PluginHook::new(&plugins, dir.path(), noop_presenter());
    let result = hook
        .execute(
            &config,
            b"{\"event\":\"pre_tool_call\",\"tool\":\"bash\",\"arguments\":{}}",
        )
        .await;
    assert_eq!(result, Ok(HookDecision::Allow));
}

#[cfg(unix)]
#[tokio::test]
async fn plugin_hook_denies_when_exit_nonzero() {
    let dir = tempdir().unwrap();
    let config = script_plugin(dir.path(), "deny_hook.sh", "echo 'forbidden operation' >&2\nexit 1\n");
    let mut plugins = BTreeMap::new();
    plugins.insert("deny_plugin".to_string(), config.clone());

    let hook = PluginHook::new(&plugins, dir.path(), noop_presenter());
    let result = hook
        .execute(
            &config,
            b"{\"event\":\"pre_tool_call\",\"tool\":\"bash\",\"arguments\":{}}",
        )
        .await;
    assert!(result.unwrap_err().contains("forbidden operation"));
}

#[cfg(unix)]
#[tokio::test]
async fn ui_prompt_round_trip_allows() {
    let dir = tempdir().unwrap();
    let config = script_plugin(
        dir.path(),
        "prompting_hook.sh",
        concat!(
            "read payload\n",
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ui/prompt\",\"params\":{\"title\":\"t\",\"body\":\"b\",\"options\":[{\"label\":\"Allow\"}]}}'\n",
            "read reply\n",
            "case \"$reply\" in\n",
            "  *'\"selected\":0'*) echo '{\"action\":\"allow\"}' ;;\n",
            "  *) echo '{\"action\":\"deny\",\"reason\":\"unexpected reply\"}' ;;\n",
            "esac\n",
        ),
    );
    let mut plugins = BTreeMap::new();
    plugins.insert("prompting".to_string(), config.clone());

    let presenter = Arc::new(ScriptedPresenter {
        response: InteractionResponse::Selected(0),
    });
    let hook = PluginHook::new(&plugins, dir.path(), presenter);
    let result = hook.execute(&config, &prompt_payload(true)).await;
    assert_eq!(result, Ok(HookDecision::Allow));
}

#[cfg(unix)]
#[tokio::test]
async fn ui_prompt_custom_text_reaches_deny_reason() {
    let dir = tempdir().unwrap();
    let config = script_plugin(
        dir.path(),
        "custom_hook.sh",
        concat!(
            "read payload\n",
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ui/prompt\",\"params\":{\"title\":\"t\",\"body\":\"b\",\"options\":[{\"label\":\"Deny\"}],\"allow_custom\":true}}'\n",
            "read reply\n",
            "case \"$reply\" in\n",
            "  *'\"custom\"'*) echo '{\"action\":\"deny\",\"reason\":\"typed by user\"}' ;;\n",
            "  *) echo '{\"action\":\"allow\"}' ;;\n",
            "esac\n",
        ),
    );
    let mut plugins = BTreeMap::new();
    plugins.insert("custom".to_string(), config.clone());

    let presenter = Arc::new(ScriptedPresenter {
        response: InteractionResponse::Custom("typed by user".to_string()),
    });
    let hook = PluginHook::new(&plugins, dir.path(), presenter);
    let result = hook.execute(&config, &prompt_payload(true)).await;
    assert_eq!(result, Err("typed by user".to_string()));
}

#[cfg(unix)]
#[tokio::test]
async fn malformed_ui_prompt_gets_a_reply_and_does_not_hang() {
    let dir = tempdir().unwrap();
    let config = script_plugin(
        dir.path(),
        "malformed_hook.sh",
        concat!(
            "read payload\n",
            "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ui/prompt\",\"params\":5}'\n",
            "read reply\n",
            "exit 0\n",
        ),
    );
    let mut plugins = BTreeMap::new();
    plugins.insert("malformed".to_string(), config.clone());

    let hook = PluginHook::new(&plugins, dir.path(), noop_presenter());
    let result = hook.execute(&config, &prompt_payload(true)).await;
    assert_eq!(result, Ok(HookDecision::Allow));
}

#[cfg(unix)]
#[tokio::test]
async fn plugin_hook_blocks_agent_tool_execution() {
    let dir = tempdir().unwrap();
    let config = script_plugin(
        dir.path(),
        "block_bash.sh",
        "echo 'denied by security plugin' >&2\nexit 1\n",
    );
    let mut plugins = BTreeMap::new();
    plugins.insert("security".to_string(), config);

    let hook = PluginHook::new(&plugins, dir.path(), noop_presenter());

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "bash", json!({"command": "rm -rf /"})),
        MockTurn::text("aborting after block"),
    ]);

    let agent = AgentBuilder::new(model.clone())
        .tool(crate::tools::BashTool::new(dir.path()))
        .add_hook(hook)
        .build();

    let response = agent.runner("test").max_turns(3).run().await.unwrap();
    assert_eq!(response.output, "aborting after block");

    let history = format!("{:?}", model.requests()[1].chat_history);
    assert!(history.contains("denied by security plugin"));
}
