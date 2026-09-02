use super::hook::DaemonHook;
use super::process::{DaemonProcess, DaemonSpawnArgs};
use super::resolve::resolve_executable;
use crate::plugin::host::HostDispatcher;
use async_trait::async_trait;
use rho_harness_core::config::PluginConfig;
use rho_harness_core::presentation::activity::ActivityToken;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::{InteractionPrompt, InteractionResponse, SessionStatus, ToolLine, WelcomeDisplay};
use rig::agent::AgentBuilder;
use rig::test_utils::{MockCompletionModel, MockTurn};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

struct MockPresenter {
    has_ui: bool,
    response: Mutex<Option<InteractionResponse>>,
}

impl MockPresenter {
    fn new(has_ui: bool, response: Option<InteractionResponse>) -> Self {
        Self {
            has_ui,
            response: Mutex::new(response),
        }
    }
}

#[async_trait]
impl Presenter for MockPresenter {
    fn write_output(&self, _text: &str) {}
    fn print_welcome(&self, _display: &WelcomeDisplay) {}
    fn print_session_status(&self, _display: &SessionStatus) {}
    fn print_notice(&self, _text: &str) {}
    fn print_user_block(&self, _input: &str) {}
    fn print_token(&self, _token: &str) {}
    fn print_thinking_token(&self, _token: &str) {}
    fn finish_tool_line(&self, _line: ToolLine) {}
    fn flush(&self) {}
    fn has_interactive_ui(&self) -> bool {
        self.has_ui
    }
    fn start_spinner(&self, _message: &str) -> ActivityToken {
        ActivityToken::default()
    }
    fn start_tool_spinner(&self, _name: &str, _arguments: &serde_json::Value) -> ActivityToken {
        ActivityToken::default()
    }
    fn start_tool_run(&self, _name: &str, _arguments: &serde_json::Value) {}
    fn stream_port(&self) -> ToolStreamPort {
        ToolStreamPort::default()
    }
    async fn request_interaction(&self, _prompt: InteractionPrompt) -> Option<InteractionResponse> {
        self.response.lock().ok().and_then(|r| r.clone())
    }
}

fn create_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let script_path = dir.join(name);
    let full = format!("#!/bin/sh\n{body}\n");
    std::fs::write(&script_path, full).expect("write script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }
    script_path
}

#[test]
fn test_resolve_executable() {
    let dir = tempdir().unwrap();
    let bin_path = create_script(dir.path(), "my_bin", "exit 0");

    let cfg_path = PluginConfig {
        path: bin_path.clone(),
        ..PluginConfig::default()
    };
    let (resolved, args) = resolve_executable(&cfg_path, dir.path()).unwrap();
    assert_eq!(resolved, bin_path);
    assert!(args.is_empty());

    let cfg_cmd = PluginConfig {
        command: Some("sh".to_string()),
        args: vec!["-c".to_string(), "exit 0".to_string()],
        ..PluginConfig::default()
    };
    let (resolved_cmd, args_cmd) = resolve_executable(&cfg_cmd, dir.path()).unwrap();
    assert_eq!(resolved_cmd, PathBuf::from("sh"));
    assert_eq!(args_cmd, vec!["-c", "exit 0"]);
}

#[tokio::test]
async fn test_daemon_process_bidirectional_rpc_and_hook() {
    let dir = tempdir().unwrap();
    let script = r#"
while IFS= read -r line; do
  case "$line" in
    *"hook/tool_call"*)
      echo '{"jsonrpc":"2.0","id":999,"method":"host/ui/confirm","params":{"title":"Confirm","message":"Allow bash?"}}'
      ;;
    *"\"confirmed\":true"*)
      echo '{"jsonrpc":"2.0","id":1,"result":{"action":"continue"}}'
      ;;
    *"\"confirmed\":false"*)
      echo '{"jsonrpc":"2.0","id":1,"result":{"action":"skip","reason":"User denied"}}'
      ;;
  esac
done
"#;
    let script_path = create_script(dir.path(), "daemon_plugin.sh", script);

    let presenter = Arc::new(MockPresenter::new(
        true,
        Some(InteractionResponse::Selected(0)), // Approved
    ));
    let dispatcher = Arc::new(HostDispatcher::new(presenter));

    let config = PluginConfig {
        path: script_path,
        enabled: true,
        ..PluginConfig::default()
    };

    let daemon = DaemonProcess::spawn(DaemonSpawnArgs {
        name: "test-daemon",
        config: &config,
        working_dir: dir.path(),
        dispatcher,
    })
    .await
    .expect("spawn daemon")
    .with_subscriptions(["tool_call"]);

    let hook = DaemonHook::from_daemons(vec![Arc::new(daemon)]);

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "bash", json!({"command": "echo 1"})),
        MockTurn::text("done"),
    ]);

    let agent = AgentBuilder::new(model.clone())
        .tool(crate::tools::BashTool::new(dir.path()))
        .add_hook(hook)
        .build();

    let response = agent.runner("test").max_turns(3).run().await.unwrap();
    assert_eq!(response.output, "done");
}

#[tokio::test]
async fn test_daemon_tool_call_skipped_when_denied() {
    let dir = tempdir().unwrap();
    let script = r#"
while IFS= read -r line; do
  case "$line" in
    *"hook/tool_call"*)
      echo '{"jsonrpc":"2.0","id":1,"result":{"action":"skip","reason":"Dangerous operation blocked"}}'
      ;;
  esac
done
"#;
    let script_path = create_script(dir.path(), "deny_daemon.sh", script);
    let presenter = Arc::new(MockPresenter::new(true, None));
    let dispatcher = Arc::new(HostDispatcher::new(presenter));

    let config = PluginConfig {
        path: script_path,
        enabled: true,
        ..PluginConfig::default()
    };

    let daemon = DaemonProcess::spawn(DaemonSpawnArgs {
        name: "deny-daemon",
        config: &config,
        working_dir: dir.path(),
        dispatcher,
    })
    .await
    .expect("spawn daemon")
    .with_subscriptions(["tool_call"]);

    let hook = DaemonHook::from_daemons(vec![Arc::new(daemon)]);

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "bash", json!({"command": "rm -rf /"})),
        MockTurn::text("aborted"),
    ]);

    let agent = AgentBuilder::new(model.clone())
        .tool(crate::tools::BashTool::new(dir.path()))
        .add_hook(hook)
        .build();

    let response = agent.runner("test").max_turns(3).run().await.unwrap();
    assert_eq!(response.output, "aborted");

    let history = format!("{:?}", model.requests()[1].chat_history);
    assert!(history.contains("Dangerous operation blocked"));
}

#[tokio::test]
async fn test_daemon_invalid_tool_repair() {
    let dir = tempdir().unwrap();
    let script = r#"
while IFS= read -r line; do
  case "$line" in
    *"hook/invalid_tool_call"*)
      echo '{"jsonrpc":"2.0","id":1,"result":{"action":"repair","tool_name":"bash"}}'
      ;;
  esac
done
"#;
    let script_path = create_script(dir.path(), "repair_daemon.sh", script);
    let presenter = Arc::new(MockPresenter::new(true, None));
    let dispatcher = Arc::new(HostDispatcher::new(presenter));

    let config = PluginConfig {
        path: script_path,
        enabled: true,
        ..PluginConfig::default()
    };

    let daemon = DaemonProcess::spawn(DaemonSpawnArgs {
        name: "repair-daemon",
        config: &config,
        working_dir: dir.path(),
        dispatcher,
    })
    .await
    .expect("spawn daemon")
    .with_subscriptions(["invalid_tool_call"]);

    let hook = DaemonHook::from_daemons(vec![Arc::new(daemon)]);

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "unknown_sh", json!({"command": "echo repaired"})),
        MockTurn::text("success"),
    ]);

    let agent = AgentBuilder::new(model.clone())
        .tool(crate::tools::BashTool::new(dir.path()))
        .add_hook(hook)
        .build();

    let response = agent.runner("test").max_turns(3).run().await.unwrap();
    assert_eq!(response.output, "success");
}
