use rho_engine::auth::AuthStore;
use rho_engine::engine::builder::AgentEngineBuilder;
use rho_engine::plugin::daemon::{DaemonHook, DaemonProcess, DaemonSpawnArgs};
use rho_engine::plugin::host::HostDispatcher;
use rho_harness_core::config::Config;
use rho_harness_core::presentation::activity::ActivityToken;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::transformer::{DisplayTransformerPipeline, ReplaceTransformer};
use rho_harness_core::presentation::{InteractionPrompt, InteractionResponse, SessionStatus, ToolLine, WelcomeDisplay};
use rig::agent::AgentBuilder;
use rig::test_utils::{MockCompletionModel, MockTurn};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

struct MockPresenter {
    has_ui: bool,
    interactive_response: Mutex<Option<InteractionResponse>>,
}

impl MockPresenter {
    fn new(has_ui: bool, response: Option<InteractionResponse>) -> Self {
        Self {
            has_ui,
            interactive_response: Mutex::new(response),
        }
    }
}

#[async_trait::async_trait]
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
        self.interactive_response.lock().ok().and_then(|r| r.clone())
    }
}

fn create_executable_script(dir: &Path, name: &str, body: &str) -> PathBuf {
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

struct TestDaemonArgs<'a> {
    dir: &'a std::path::Path,
    name: &'a str,
    script: &'a str,
    interaction: Option<InteractionResponse>,
    subscriptions: &'a [&'a str],
}

async fn spawn_integration_daemon(args: TestDaemonArgs<'_>) -> (DaemonHook, Arc<MockPresenter>) {
    let script_path = create_executable_script(args.dir, &format!("{}.sh", args.name), args.script);
    let presenter = Arc::new(MockPresenter::new(true, args.interaction));
    let dispatcher = Arc::new(HostDispatcher::new(presenter.clone()));
    let config = rho_harness_core::config::PluginConfig {
        path: script_path,
        enabled: true,
        ..Default::default()
    };
    let daemon = DaemonProcess::spawn(DaemonSpawnArgs {
        name: args.name,
        config: &config,
        working_dir: args.dir,
        dispatcher,
    })
    .await
    .expect("spawn daemon")
    .with_subscriptions(args.subscriptions.iter().copied());
    (DaemonHook::from_daemons(vec![Arc::new(daemon)]), presenter)
}

#[tokio::test]
async fn test_decoupled_permission_plugin_with_interactive_modal() {
    let dir = tempdir().unwrap();
    let script = "while IFS= read -r line; do case \"$line\" in *\"hook/tool_call\"*) echo '{\"jsonrpc\":\"2.0\",\"id\":100,\"method\":\"host/ui/confirm\",\"params\":{\"title\":\"Prompt\",\"message\":\"Exec?\"}}' ;; *\"\\\"confirmed\\\":true\"*) echo '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"action\":\"continue\"}}' ;; *\"\\\"confirmed\\\":false\"*) echo '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"action\":\"skip\",\"reason\":\"User denied command execution\"}}' ;; esac; done\n";
    let (hook, _) = spawn_integration_daemon(TestDaemonArgs {
        dir: dir.path(),
        name: "permission_plugin",
        script,
        interaction: Some(InteractionResponse::Selected(0)),
        subscriptions: &["tool_call"],
    })
    .await;

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "bash", json!({"command": "git status"})),
        MockTurn::text("status check done"),
    ]);

    let agent = AgentBuilder::new(model)
        .tool(rho_engine::tools::BashTool::new(dir.path()))
        .add_hook(hook)
        .build();
    let response = agent.runner("check status").max_turns(3).run().await.unwrap();
    assert_eq!(response.output, "status check done");
}

fn dynamic_image_tool() -> rig::tool::DynamicTool {
    rig::tool::DynamicTool::new(
        "generate_image",
        "Generate image from prompt",
        json!({"type": "object", "properties": { "prompt": { "type": "string" } }, "required": ["prompt"]}),
        |_ctx, _args| Box::pin(async { Ok(rig::tool::ToolOutput::text("generated: output.png")) }),
    )
}

#[tokio::test]
async fn test_dynamic_plugin_tool_registration_and_execution() {
    let dir = tempdir().unwrap();
    let config = Config {
        config_dir: dir.path().to_path_buf(),
        sessions_dir: dir.path().join("sessions"),
        auth_file: dir.path().join("auth.json"),
        ..Config::default()
    };
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let engine = AgentEngineBuilder::new(config, auth_store)
        .base_dir(dir.path().to_path_buf())
        .add_tool(dynamic_image_tool())
        .build()
        .await
        .unwrap();
    assert!(
        engine.tool_names().contains(&"generate_image".to_string())
            && engine.tool_names().contains(&"read".to_string())
    );
}

fn assert_alias_transform(before: &str, expected: &str) {
    let mut pipeline = DisplayTransformerPipeline::new();
    pipeline.add(Arc::new(ReplaceTransformer::new("mcp__bash__run", "bash")));
    assert_eq!(pipeline.transform(before), expected);
}

#[tokio::test]
async fn test_claude_code_compatibility_aliasing_and_repair_flow() {
    let dir = tempdir().unwrap();
    let script = "while IFS= read -r line; do case \"$line\" in *\"hook/invalid_tool_call\"*) echo '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"action\":\"repair\",\"tool_name\":\"bash\"}}' ;; esac; done\n";
    let (hook, _) = spawn_integration_daemon(TestDaemonArgs {
        dir: dir.path(),
        name: "claude_code_plugin",
        script,
        interaction: None,
        subscriptions: &["invalid_tool_call"],
    })
    .await;
    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "mcp__bash__run", json!({"command": "echo aliased"})),
        MockTurn::text("success"),
    ]);

    let agent = AgentBuilder::new(model)
        .tool(rho_engine::tools::BashTool::new(dir.path()))
        .add_hook(hook)
        .build();
    let response = agent.runner("run aliased").max_turns(3).run().await.unwrap();
    assert_eq!(response.output, "success");
    assert_alias_transform(
        "Model invoked `mcp__bash__run` with success.",
        "Model invoked `bash` with success.",
    );
}

struct NativeTestPlugin;

impl rho_engine::plugin::RhoPlugin for NativeTestPlugin {
    fn name(&self) -> &str {
        "native_test_plugin"
    }

    fn tools(&self) -> Vec<rig::tool::DynamicTool> {
        vec![rig::tool::DynamicTool::new(
            "custom_eval",
            "Custom evaluation tool",
            json!({
                "type": "object",
                "properties": { "expression": { "type": "string" } },
                "required": ["expression"]
            }),
            |_ctx, _args| Box::pin(async { Ok(rig::tool::ToolOutput::text("result: 42")) }),
        )]
    }

    fn register_hooks(&self, stack: &mut rig::agent::hook::HookStack) {
        struct NativeGuardHook;
        impl rig::agent::hook::AgentHook for NativeGuardHook {
            async fn on_tool_call(
                &self,
                _ctx: &rig::agent::hook::HookContext,
                event: rig::agent::hook::ToolCall<'_>,
            ) -> rig::agent::hook::ToolCallAction {
                if event.tool_name == "dangerous_op" {
                    return rig::agent::hook::ToolCallAction::skip("Blocked by native guard hook");
                }
                rig::agent::hook::ToolCallAction::run()
            }
        }
        stack.push(NativeGuardHook);
    }
}

#[tokio::test]
async fn test_native_in_process_rho_plugin() {
    let dir = tempdir().unwrap();
    let config = Config {
        config_dir: dir.path().to_path_buf(),
        sessions_dir: dir.path().join("sessions"),
        auth_file: dir.path().join("auth.json"),
        ..Config::default()
    };
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();

    let plugin = Arc::new(NativeTestPlugin);
    let engine = AgentEngineBuilder::new(config, auth_store)
        .base_dir(dir.path().to_path_buf())
        .plugin(plugin)
        .build()
        .await
        .unwrap();

    assert!(engine.tool_names().contains(&"custom_eval".to_string()));
    assert!(engine.tool_names().contains(&"read".to_string()));
}

#[tokio::test]
async fn test_rag_document_injection_via_plugin() {
    let dir = tempdir().unwrap();
    let script = "while IFS= read -r line; do case \"$line\" in *\"hook/completion_call\"*) echo '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"action\":\"override_request\",\"request\":{\"extra_context\":[{\"id\":\"arch.md\",\"text\":\"Strict layering required\"}]}}}' ;; esac; done\n";
    let (hook, _) = spawn_integration_daemon(TestDaemonArgs {
        dir: dir.path(),
        name: "rag_plugin",
        script,
        interaction: None,
        subscriptions: &["completion_call"],
    })
    .await;
    let model = MockCompletionModel::new([MockTurn::text("understood guidelines")]);
    let agent = AgentBuilder::new(model.clone()).add_hook(hook).build();

    let response = agent.runner("explain arch").run().await.unwrap();
    assert_eq!(response.output, "understood guidelines");

    let requests = model.requests();
    let docs = &requests[0].documents;
    assert_eq!(
        (docs.len(), docs[0].id.as_str(), docs[0].text.as_str()),
        (1, "arch.md", "Strict layering required")
    );
}

#[tokio::test]
async fn test_plugin_block_and_status_dispatch() {
    let dir = tempdir().unwrap();
    let script = "while IFS= read -r line; do case \"$line\" in *\"hook/completion_response\"*) echo '{\"jsonrpc\":\"2.0\",\"id\":10,\"method\":\"host/ui/block\",\"params\":{\"title\":\"Audit Report\",\"content\":\"Clean\",\"style\":\"success\"}}'; echo '{\"jsonrpc\":\"2.0\",\"id\":11,\"method\":\"host/ui/set_status\",\"params\":{\"key\":\"quota\",\"text\":\"5h: 80%\"}}'; echo '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"action\":\"continue\"}}' ;; esac; done\n";
    let (hook, _) = spawn_integration_daemon(TestDaemonArgs {
        dir: dir.path(),
        name: "status_plugin",
        script,
        interaction: None,
        subscriptions: &["completion_response"],
    })
    .await;
    let model = MockCompletionModel::new([MockTurn::text("done")]);
    let agent = AgentBuilder::new(model).add_hook(hook).build();

    let response = agent.runner("test").run().await.unwrap();
    assert_eq!(response.output, "done");
}
