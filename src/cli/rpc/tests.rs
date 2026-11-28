use std::sync::Arc;
use tokio::io::duplex;
use tokio::sync::RwLock;

use super::handlers::auth::handle_set_api_key_cmd;
use super::handlers::config::{handle_config_update_cmd, handle_node_info_cmd, handle_state_command};
use super::handlers::session::{extract_chat_messages, handle_create_session_cmd, handle_get_tree_cmd};
use super::handlers::turn::{handle_tool_response_cmd, parse_tool_decision};
use super::handlers::{handle_node_session_auth_cmd, handle_resume_or_fork_cmd};
use super::presenter::{RpcEventQueue, RpcPresenter};
use super::types::RpcDaemonContext;
use crate::auth::AuthStore;
use crate::config::Config;
use crate::steering::SharedSteeringQueue;
use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::types::InteractionResponse;
use rho_harness_core::presentation::{InteractionOption, InteractionPrompt, OptionLayout};
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent, RpcResponse};
use rho_harness_core::rpc::transport::{JsonLinesReader, JsonLinesWriter};

#[test]
fn test_parse_tool_decision_mappings() {
    assert_eq!(parse_tool_decision("allow"), InteractionResponse::Selected(0));
    assert_eq!(parse_tool_decision("0"), InteractionResponse::Selected(0));
    assert_eq!(parse_tool_decision("always"), InteractionResponse::Selected(2));
    assert_eq!(parse_tool_decision("always_allow"), InteractionResponse::Selected(2));
    assert_eq!(parse_tool_decision("deny"), InteractionResponse::Cancelled);
    assert_eq!(parse_tool_decision("cancel"), InteractionResponse::Cancelled);
    assert_eq!(
        parse_tool_decision("edit: echo hello"),
        InteractionResponse::SelectedWithInput {
            index: 1,
            text: "echo hello".to_string(),
        }
    );
    assert_eq!(
        parse_tool_decision("deny: security policy violation"),
        InteractionResponse::SelectedWithInput {
            index: 3,
            text: "security policy violation".to_string(),
        }
    );
}

#[test]
fn unknown_tool_decisions_never_approve() {
    for decision in [
        "reject", "no", "nope", "", "  ", "1", "3", "allow!", "yes", "edit", "deny-ish",
    ] {
        assert_eq!(
            parse_tool_decision(decision),
            InteractionResponse::Cancelled,
            "{decision:?}"
        );
    }
    assert_eq!(parse_tool_decision("  ALLOW "), InteractionResponse::Selected(0));
}

#[tokio::test]
async fn test_rpc_presenter_tool_approval_roundtrip() {
    let events = RpcEventQueue::default();
    let presenter = RpcPresenter::new(events.clone());
    let pending = presenter.pending_approvals();

    let prompt = InteractionPrompt {
        title: "bash".to_string(),
        body: "rm -rf target".to_string(),
        options: vec![InteractionOption {
            label: "Allow".to_string(),
            description: Some("Run command".to_string()),
            input: None,
        }],
        initial_selection: 0,
        allow_custom: false,
        initial_text: None,
        option_layout: OptionLayout::Vertical,
    };

    let pres_clone = presenter.clone();
    let handle = tokio::spawn(async move { pres_clone.request_interaction(prompt).await });

    let mut emitted = Vec::new();
    while emitted.len() < 2 {
        emitted.extend(events.next_batch().await.expect("the queue stays open"));
    }
    let approval_id = match &emitted[0] {
        RpcEvent::ToolApprovalRequest { approval_id, tool, .. } => {
            assert_eq!(tool, "bash");
            approval_id.clone()
        }
        other => panic!("expected ToolApprovalRequest, got {other:?}"),
    };
    assert_eq!(
        emitted[1],
        RpcEvent::StatusChanged {
            status: "waiting_approval".to_string()
        }
    );

    // 3. Resolve approval via handle_tool_response_cmd
    let (_client_io, server_io) = duplex(1024);
    let mut writer = JsonLinesWriter::new(server_io);
    let temp_dir = rho_test_support::TestDir::new();
    let config = Config {
        sessions_dir: temp_dir.join("sessions"),
        auth_file: temp_dir.join("auth.json"),
        ..Config::default()
    };
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), Default::default(), None)
        .await
        .unwrap();
    let engine_lock = Arc::new(RwLock::new(engine));
    let config_lock = Arc::new(RwLock::new(config));
    let auth_store_lock = Arc::new(RwLock::new(auth_store));
    let steering = Arc::new(SharedSteeringQueue::new(rho_engine::engine::runner::QueueMode::All));
    let mut active_turn = None;

    let pres_arc: Arc<dyn Presenter> = Arc::new(presenter);
    let mut ctx = RpcDaemonContext {
        writer: &mut writer,
        engine: engine_lock,
        presenter: pres_arc,
        config: config_lock,
        auth_store: auth_store_lock,
        pending_approvals: pending,
        steering,
        active_turn: &mut active_turn,
        events,
        auth_bridge: rho_harness_core::rpc::RpcAuthBridge::new(),
        live_models: false,
    };

    handle_tool_response_cmd(approval_id, "allow".to_string(), None, &mut ctx)
        .await
        .unwrap();

    let response = handle.await.unwrap();
    assert_eq!(response, Some(InteractionResponse::Selected(0)));
}

async fn setup_test_rpc_context<'a, W: tokio::io::AsyncWrite + Unpin>(
    writer: &'a mut JsonLinesWriter<W>,
    temp_dir: &std::path::Path,
    active_turn: &'a mut Option<tokio::task::JoinHandle<crate::error::Result<rho_engine::engine::runner::TurnOutput>>>,
) -> RpcDaemonContext<'a, W> {
    let events = RpcEventQueue::default();
    let presenter = RpcPresenter::new(events.clone());
    let pending = presenter.pending_approvals();
    let config = Config {
        sessions_dir: temp_dir.join("sessions"),
        auth_file: temp_dir.join("auth.json"),
        model: "mock-model".to_string(),
        provider: "local".to_string(),
        ..Config::default()
    };
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), Default::default(), None)
        .await
        .unwrap();
    RpcDaemonContext {
        writer,
        engine: Arc::new(RwLock::new(engine)),
        presenter: Arc::new(presenter),
        config: Arc::new(RwLock::new(config)),
        auth_store: Arc::new(RwLock::new(auth_store)),
        pending_approvals: pending,
        steering: Arc::new(SharedSteeringQueue::new(rho_engine::engine::runner::QueueMode::All)),
        active_turn,
        events,
        auth_bridge: rho_harness_core::rpc::RpcAuthBridge::new(),
        live_models: false,
    }
}

type ClientReader = JsonLinesReader<tokio::io::BufReader<tokio::io::DuplexStream>>;

fn rpc_fixture(
    prefix: &str,
) -> (
    ClientReader,
    JsonLinesWriter<tokio::io::DuplexStream>,
    rho_test_support::TestDir,
) {
    let (client_io, server_io) = duplex(4096);
    let temp_dir = rho_test_support::TestDir::with_prefix(&format!("{prefix}_"));
    (
        JsonLinesReader::new(tokio::io::BufReader::new(client_io)),
        JsonLinesWriter::new(server_io),
        temp_dir,
    )
}

async fn read_response(reader: &mut ClientReader) -> RpcResponse {
    reader.read_message::<RpcResponse>().await.unwrap().unwrap()
}

#[tokio::test]
async fn test_rpc_state_command_and_get_tree() {
    let (mut client_reader, mut writer, temp_dir) = rpc_fixture("rpc_tree_test");
    let mut active_turn = None;
    let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;

    handle_state_command(Some("req-state".to_string()), &mut ctx)
        .await
        .unwrap();
    handle_get_tree_cmd(Some("req-tree".to_string()), &mut ctx)
        .await
        .unwrap();
    handle_node_info_cmd(Some("req-info".to_string()), &mut ctx)
        .await
        .unwrap();
    handle_create_session_cmd(
        Some(temp_dir.display().to_string()),
        Some("req-create".to_string()),
        &mut ctx,
    )
    .await
    .unwrap();

    let resp1 = read_response(&mut client_reader).await;
    assert_eq!(resp1.id, Some("req-state".to_string()));
    let state_data = resp1.data.unwrap();
    assert!(state_data.get("active_workspace").is_some());
    assert!(state_data.get("total_input_tokens").is_some());
    assert!(state_data.get("context_window").is_some());

    let resp2 = read_response(&mut client_reader).await;
    assert_eq!(resp2.command, "get_tree");
    assert!(resp2.data.unwrap().get("entries").is_some());
    let _resp3 = read_response(&mut client_reader).await;

    let resp4 = read_response(&mut client_reader).await;
    assert_eq!(resp4.id, Some("req-create".to_string()));
    let create_data = resp4.data.unwrap();
    assert!(create_data.get("active_workspace").is_some());
    assert!(create_data.get("total_input_tokens").is_some());
}

#[tokio::test]
async fn test_rpc_config_update_handlers() {
    let (mut client_reader, mut writer, temp_dir) = rpc_fixture("rpc_cfg_test");
    let mut active_turn = None;
    let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;

    handle_config_update_cmd(
        RpcCommand::SetThinking {
            level: "high".to_string(),
        },
        Some("req-thinking".to_string()),
        &mut ctx,
    )
    .await
    .unwrap();

    handle_config_update_cmd(
        RpcCommand::SetThinking {
            level: "ultra".to_string(),
        },
        Some("req-thinking-invalid".to_string()),
        &mut ctx,
    )
    .await
    .unwrap();

    handle_config_update_cmd(
        RpcCommand::SetModel {
            model: "llama3.2".to_string(),
            provider: Some("local".to_string()),
        },
        Some("req-model-success".to_string()),
        &mut ctx,
    )
    .await
    .unwrap();

    handle_config_update_cmd(
        RpcCommand::SetModel {
            model: "unknown-model".to_string(),
            provider: Some("nonexistent-provider-xyz".to_string()),
        },
        Some("req-model-failure".to_string()),
        &mut ctx,
    )
    .await
    .unwrap();

    handle_config_update_cmd(
        RpcCommand::Compact {
            instructions: Some("compact please".to_string()),
        },
        Some("req-compact".to_string()),
        &mut ctx,
    )
    .await
    .unwrap();

    handle_config_update_cmd(RpcCommand::GetState, Some("req-noop".to_string()), &mut ctx)
        .await
        .unwrap();

    let resp_thinking = read_response(&mut client_reader).await;
    assert_eq!(resp_thinking.id, Some("req-thinking".to_string()));
    assert!(resp_thinking.success);

    let resp_invalid = read_response(&mut client_reader).await;
    assert_eq!(resp_invalid.id, Some("req-thinking-invalid".to_string()));
    assert!(!resp_invalid.success);
    assert_eq!(ctx.config.read().await.thinking_level.as_deref(), Some("high"));

    let resp_model_ok = read_response(&mut client_reader).await;
    assert_eq!(resp_model_ok.id, Some("req-model-success".to_string()));
    assert!(resp_model_ok.success);

    let resp_model_err = read_response(&mut client_reader).await;
    assert_eq!(resp_model_err.id, Some("req-model-failure".to_string()));
    assert!(!resp_model_err.success);
    assert_eq!(ctx.config.read().await.canonical_model_spec(), "local/llama3.2");

    let resp_compact = read_response(&mut client_reader).await;
    assert_eq!(resp_compact.id, Some("req-compact".to_string()));
}

#[tokio::test]
async fn set_model_keeps_a_bare_id_on_the_provider_that_lists_it() {
    let (mut client_reader, mut writer, temp_dir) = rpc_fixture("rpc_set_model_listed");
    let mut active_turn = None;
    let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;
    let listed = rho_engine::provider::DiscoveredModel::new("gpt-oss:20b", "gpt-oss:20b", "");
    ctx.engine
        .read()
        .await
        .model_catalog()
        .set("local", rho_engine::provider::ProviderModels::Ready(vec![listed]));

    handle_config_update_cmd(
        RpcCommand::SetModel {
            model: "gpt-oss:20b".to_string(),
            provider: None,
        },
        Some("req-listed".to_string()),
        &mut ctx,
    )
    .await
    .unwrap();

    let resp = read_response(&mut client_reader).await;
    assert!(resp.success, "{:?}", resp.error);
    assert_eq!(ctx.config.read().await.canonical_model_spec(), "local/gpt-oss:20b");
    assert_eq!(ctx.engine.read().await.config.provider, "local");
}

#[tokio::test]
async fn engine_changes_are_rejected_while_a_turn_is_running() {
    let (mut client_reader, mut writer, temp_dir) = rpc_fixture("rpc_compact_busy");
    let mut active_turn = Some(tokio::spawn(std::future::pending::<
        crate::error::Result<rho_engine::engine::runner::TurnOutput>,
    >()));
    let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;
    let commands = [
        (RpcCommand::Compact { instructions: None }, "compact"),
        (
            RpcCommand::SetModel {
                model: "llama3.2".to_string(),
                provider: Some("local".to_string()),
            },
            "set_model",
        ),
        (
            RpcCommand::SetThinking {
                level: "high".to_string(),
            },
            "set_thinking",
        ),
    ];

    for (command, name) in commands {
        handle_config_update_cmd(command, None, &mut ctx).await.unwrap();
        let resp = read_response(&mut client_reader).await;
        assert_eq!(resp.command, name);
        assert!(!resp.success);
        assert!(resp.error.unwrap().contains("turn is in progress"));
    }
}

#[test]
fn test_extract_chat_messages_preserves_tools() {
    use rho_harness_core::model::{
        AssistantContent, ChatMessage, TextContent, ToolCall, ToolFunction, ToolResult, ToolResultContent, UserContent,
    };
    let msgs = vec![
        ChatMessage::user("run check"),
        ChatMessage::Assistant {
            id: None,
            content: vec![
                AssistantContent::Text(TextContent::new("I will run the command.")),
                AssistantContent::ToolCall(ToolCall::new(
                    "call_1",
                    ToolFunction::new("bash", serde_json::json!({ "command": "cargo check" })),
                )),
            ],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: "call_1".to_string(),
                provider: None,
                name: "bash".to_string(),
                content: vec![ToolResultContent::Text(TextContent::new(
                    "Finished dev [unoptimized + debuginfo]",
                ))],
            })],
        },
        ChatMessage::assistant("Check passed cleanly."),
    ];

    let extracted = extract_chat_messages(&msgs);
    assert_eq!(extracted.len(), 4);
    assert_eq!(extracted[0]["role"], "user");
    assert_eq!(extracted[0]["content"], "run check");
    assert_eq!(extracted[1]["role"], "assistant");
    assert_eq!(extracted[1]["content"], "I will run the command.");
    assert_eq!(extracted[2]["role"], "tool");
    assert_eq!(extracted[2]["tool"], "bash");
    assert_eq!(extracted[2]["arguments"]["command"], "cargo check");
    assert_eq!(extracted[2]["output"], "Finished dev [unoptimized + debuginfo]");
    assert_eq!(extracted[3]["role"], "assistant");
    assert_eq!(extracted[3]["content"], "Check passed cleanly.");
}

async fn send_and_expect_success<R: tokio::io::AsyncBufRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut JsonLinesWriter<W>,
    reader: &mut JsonLinesReader<R>,
    id: &str,
    cmd: rho_harness_core::rpc::protocol::RpcCommand,
    expected_command: &str,
) {
    let req = rho_harness_core::rpc::protocol::RpcRequest {
        id: Some(id.to_string()),
        command: cmd,
    };
    writer.write_message(&req).await.unwrap();
    let resp = reader.read_message::<RpcResponse>().await.unwrap().unwrap();
    assert_eq!(resp.id, Some(id.to_string()));
    assert_eq!(resp.command, expected_command);
    assert!(resp.success);
}

async fn setup_rpc_test_server(
    server_read: tokio::io::ReadHalf<tokio::io::DuplexStream>,
    server_write: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    temp_dir: &std::path::Path,
) -> tokio::task::JoinHandle<crate::error::Result<()>> {
    let config = Config {
        sessions_dir: temp_dir.join("sessions"),
        auth_file: temp_dir.join("auth.json"),
        ..Config::default()
    };
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), Default::default(), None)
        .await
        .unwrap();
    let engine_lock = Arc::new(RwLock::new(engine));
    let config_lock = Arc::new(RwLock::new(config));
    let auth_store_lock = Arc::new(RwLock::new(auth_store));

    tokio::spawn(async move {
        super::run_rpc_session_over_stream(server_read, server_write, engine_lock, config_lock, auth_store_lock).await
    })
}

#[tokio::test]
async fn test_run_rpc_session_over_stream_roundtrip() {
    let (client_io, server_io) = duplex(65536);
    let (server_read, server_write) = tokio::io::split(server_io);
    let (client_read, client_write) = tokio::io::split(client_io);

    let temp_dir = rho_test_support::TestDir::new();
    let handle = setup_rpc_test_server(server_read, server_write, &temp_dir).await;

    let mut writer = JsonLinesWriter::new(client_write);
    let mut reader = JsonLinesReader::new(tokio::io::BufReader::new(client_read));

    let first = reader.read_message::<RpcEvent>().await.unwrap().unwrap();
    assert!(matches!(first, RpcEvent::SessionStart { .. }));

    send_and_expect_success(
        &mut writer,
        &mut reader,
        "req-node",
        rho_harness_core::rpc::protocol::RpcCommand::GetNodeInfo,
        "get_node_info",
    )
    .await;

    send_and_expect_success(
        &mut writer,
        &mut reader,
        "req-state",
        rho_harness_core::rpc::protocol::RpcCommand::GetState,
        "get_state",
    )
    .await;

    let req3 = rho_harness_core::rpc::protocol::RpcRequest {
        id: Some("req-abort".to_string()),
        command: rho_harness_core::rpc::protocol::RpcCommand::Abort,
    };
    writer.write_message(&req3).await.unwrap();
    let ev_abort = reader.read_message::<RpcEvent>().await.unwrap().unwrap();
    assert!(matches!(ev_abort, RpcEvent::StatusChanged { .. }));
    let resp3 = reader.read_message::<RpcResponse>().await.unwrap().unwrap();
    assert_eq!(resp3.command, "abort");
    assert!(resp3.success);

    send_and_expect_success(
        &mut writer,
        &mut reader,
        "req-steer",
        rho_harness_core::rpc::protocol::RpcCommand::Steer {
            message: "steer msg".into(),
        },
        "steer",
    )
    .await;

    send_and_expect_success(
        &mut writer,
        &mut reader,
        "req-tree",
        rho_harness_core::rpc::protocol::RpcCommand::GetTree,
        "get_tree",
    )
    .await;

    send_and_expect_success(
        &mut writer,
        &mut reader,
        "req-list",
        rho_harness_core::rpc::protocol::RpcCommand::ListSessions,
        "list_sessions",
    )
    .await;

    send_and_expect_success(
        &mut writer,
        &mut reader,
        "req-exit",
        rho_harness_core::rpc::protocol::RpcCommand::Exit,
        "exit",
    )
    .await;

    drop(writer);
    drop(reader);
    let _ = handle.await;
}

enum RpcHandler {
    NodeSessionAuth,
    ResumeOrFork,
}

enum RpcExpect {
    Unhandled,
    Reply {
        command: Option<&'static str>,
        check: fn(&RpcResponse),
    },
}

struct RpcStep {
    handler: RpcHandler,
    cmd: RpcCommand,
    id: &'static str,
    expect: RpcExpect,
}

fn expect_ok(resp: &RpcResponse) {
    assert!(resp.success);
}

fn expect_failure(resp: &RpcResponse) {
    assert!(!resp.success);
}

fn expect_unknown_provider(resp: &RpcResponse) {
    assert!(!resp.success);
    assert!(resp.error.as_deref().unwrap().contains("Unknown provider"));
}

fn expect_input_unresolved(resp: &RpcResponse) {
    assert!(resp.success);
    assert_eq!(resp.data.as_ref().unwrap()["resolved"], false);
}

async fn run_rpc_step(
    ctx: &mut RpcDaemonContext<'_, tokio::io::DuplexStream>,
    reader: &mut ClientReader,
    step: RpcStep,
) -> Option<RpcResponse> {
    let req_id = Some(step.id.to_string());
    let handled = match step.handler {
        RpcHandler::NodeSessionAuth => handle_node_session_auth_cmd(&step.cmd, req_id, ctx).await,
        RpcHandler::ResumeOrFork => handle_resume_or_fork_cmd(&step.cmd, req_id, ctx).await,
    }
    .unwrap();
    match step.expect {
        RpcExpect::Unhandled => {
            assert!(!handled);
            None
        }
        RpcExpect::Reply { command, check } => {
            assert!(handled);
            let resp = read_response(reader).await;
            if let Some(command) = command {
                assert_eq!(resp.command, command);
            }
            check(&resp);
            Some(resp)
        }
    }
}

#[tokio::test]
async fn test_handle_node_session_auth_and_resume_cmds() {
    let (mut client_reader, mut writer, temp_dir) = rpc_fixture("rpc_auth_fork_test");
    let mut active_turn = None;
    let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;

    let fork = run_rpc_step(
        &mut ctx,
        &mut client_reader,
        RpcStep {
            handler: RpcHandler::ResumeOrFork,
            cmd: RpcCommand::ForkSession { node_id: None },
            id: "req-fork",
            expect: RpcExpect::Reply {
                command: Some("fork_session"),
                check: expect_ok,
            },
        },
    )
    .await
    .unwrap();
    let forked_sid = fork.data.unwrap()["session_id"].as_str().unwrap().to_string();

    let steps = [
        RpcStep {
            handler: RpcHandler::ResumeOrFork,
            cmd: RpcCommand::ResumeSession { session_id: forked_sid },
            id: "req-resume",
            expect: RpcExpect::Reply {
                command: Some("resume_session"),
                check: expect_ok,
            },
        },
        RpcStep {
            handler: RpcHandler::ResumeOrFork,
            cmd: RpcCommand::ResumeSession {
                session_id: "nonexistent-session-id-12345".to_string(),
            },
            id: "req-resume-fail",
            expect: RpcExpect::Reply {
                command: Some("resume_session"),
                check: expect_failure,
            },
        },
        RpcStep {
            handler: RpcHandler::NodeSessionAuth,
            cmd: RpcCommand::AuthLogin {
                provider: "invalid_prov_xyz".to_string(),
            },
            id: "req-login-fail",
            expect: RpcExpect::Reply {
                command: None,
                check: expect_unknown_provider,
            },
        },
        RpcStep {
            handler: RpcHandler::NodeSessionAuth,
            cmd: RpcCommand::AuthLogin {
                provider: "openai".to_string(),
            },
            id: "req-login-ok",
            expect: RpcExpect::Reply {
                command: None,
                check: expect_ok,
            },
        },
        RpcStep {
            handler: RpcHandler::NodeSessionAuth,
            cmd: RpcCommand::AuthInput {
                interaction_id: "nonexistent".to_string(),
                secret_value: Some("sec".to_string()),
                selected_option: None,
            },
            id: "req-input",
            expect: RpcExpect::Reply {
                command: None,
                check: expect_input_unresolved,
            },
        },
        RpcStep {
            handler: RpcHandler::NodeSessionAuth,
            cmd: RpcCommand::GetState,
            id: "req-state",
            expect: RpcExpect::Unhandled,
        },
        RpcStep {
            handler: RpcHandler::NodeSessionAuth,
            cmd: RpcCommand::GetNodeInfo,
            id: "req-info",
            expect: RpcExpect::Reply {
                command: Some("get_node_info"),
                check: expect_ok,
            },
        },
        RpcStep {
            handler: RpcHandler::NodeSessionAuth,
            cmd: RpcCommand::CreateSession { workspace: None },
            id: "req-create",
            expect: RpcExpect::Reply {
                command: Some("create_session"),
                check: expect_ok,
            },
        },
        RpcStep {
            handler: RpcHandler::NodeSessionAuth,
            cmd: RpcCommand::SetApiKey {
                provider: "anthropic".to_string(),
                api_key: "sk-ant-testkey".to_string(),
            },
            id: "req-key-ok",
            expect: RpcExpect::Reply {
                command: None,
                check: expect_ok,
            },
        },
        RpcStep {
            handler: RpcHandler::ResumeOrFork,
            cmd: RpcCommand::GetState,
            id: "req-state",
            expect: RpcExpect::Unhandled,
        },
    ];
    for step in steps {
        run_rpc_step(&mut ctx, &mut client_reader, step).await;
    }

    assert!(ctx.auth_store.read().await.get_credential("anthropic").is_some());
}

#[tokio::test]
async fn switching_to_an_unknown_node_over_rpc_reports_failure() {
    use super::handlers::handle_session_lifecycle_cmd;
    use rho_harness_core::rpc::protocol::RpcCommand;

    let (mut client_reader, mut writer, temp_dir) = rpc_fixture("rpc_switch_unknown");
    let mut active_turn = None;
    let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;
    let cmd = RpcCommand::SwitchBranch {
        node_id: "nonexistent-node-xyz".to_string(),
    };

    assert!(
        handle_session_lifecycle_cmd(&cmd, Some("req-switch".into()), &mut ctx)
            .await
            .unwrap()
    );

    let resp = read_response(&mut client_reader).await;
    assert_eq!(resp.command, "switch_branch");
    assert!(!resp.success);
    assert!(resp.error.unwrap().contains("nonexistent-node-xyz"));
}

#[tokio::test]
async fn steering_over_rpc_reports_failure_once_the_queue_is_full() {
    use super::handlers::turn::handle_steer_command;
    use rho_harness_core::queue::PENDING_QUEUE_CAPACITY;

    let (mut client_reader, mut writer, temp_dir) = rpc_fixture("rpc_steer_full");
    let mut active_turn = None;
    let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;
    for n in 0..PENDING_QUEUE_CAPACITY {
        ctx.steering.enqueue(format!("steer {n}")).unwrap();
    }

    handle_steer_command("one too many".into(), Some("req-steer".into()), &mut ctx)
        .await
        .unwrap();

    let resp = read_response(&mut client_reader).await;
    assert_eq!(resp.command, "steer");
    assert!(!resp.success);
    assert!(resp.error.unwrap().contains("queue is full"));
}

#[tokio::test]
async fn test_handle_session_lifecycle_cmd_variants() {
    use super::handlers::handle_session_lifecycle_cmd;
    use rho_harness_core::rpc::protocol::RpcCommand;

    let (mut client_reader, mut writer, temp_dir) = rpc_fixture("rpc_lifecycle_test");
    let mut active_turn = None;
    let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;

    let steps = [
        (RpcCommand::GetTree, "req-tree", "get_tree"),
        (
            RpcCommand::SetNodeLabel {
                node_id: "root-1".to_string(),
                label: Some("test label".to_string()),
            },
            "req-label",
            "set_node_label",
        ),
        (RpcCommand::ListSessions, "req-list", "list_sessions"),
    ];
    for (cmd, req_id, command) in steps {
        assert!(
            handle_session_lifecycle_cmd(&cmd, Some(req_id.to_string()), &mut ctx)
                .await
                .unwrap()
        );
        let resp = read_response(&mut client_reader).await;
        assert!(resp.success);
        assert_eq!(resp.command, command);
    }

    let res = handle_session_lifecycle_cmd(&RpcCommand::GetState, Some("req-state".to_string()), &mut ctx).await;
    assert!(!res.unwrap());
}

#[tokio::test]
async fn test_handle_create_session_workspace_target() {
    let (mut client_reader, mut writer, temp_dir) = rpc_fixture("rpc_create_session_target_test");
    let workspace = temp_dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut active_turn = None;
    let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;

    handle_create_session_cmd(
        Some(workspace.display().to_string()),
        Some("req-create".to_string()),
        &mut ctx,
    )
    .await
    .unwrap();

    let resp = read_response(&mut client_reader).await;
    assert!(resp.success);
    assert_eq!(resp.command, "create_session");
    assert_eq!(resp.data.unwrap()["active_workspace"].as_str(), workspace.to_str());
    assert_eq!(ctx.engine.read().await.base_dir, workspace);
    assert_eq!(ctx.config.read().await.sessions_dir, workspace.join(".rho/sessions"));
}

#[tokio::test]
async fn test_handle_set_api_key_cmd_outcomes() {
    let cases = [
        ("saved", false, None),
        ("save_blocked", true, Some("Failed to read auth file")),
    ];
    for (name, blocked, expected_error) in cases {
        let (mut client_reader, mut writer, temp_dir) = rpc_fixture("rpc_set_api_key_test");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let path = temp_dir.join("auth-under-test.json");
        let store = AuthStore::load(&path).unwrap();
        if blocked {
            std::fs::create_dir(&path).unwrap();
        }
        let mut active_turn = None;
        let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;
        *ctx.auth_store.write().await = store;

        handle_set_api_key_cmd(
            "anthropic".to_string(),
            "sk-ant-testkey".to_string(),
            Some("req-key".to_string()),
            &mut ctx,
        )
        .await
        .unwrap();

        let resp = read_response(&mut client_reader).await;
        assert_eq!(resp.command, "set_api_key", "{name}");
        assert_eq!(resp.success, expected_error.is_none(), "{name}");
        match expected_error {
            Some(needle) => assert!(resp.error.unwrap().contains(needle), "{name}"),
            None => assert!(
                ctx.auth_store.read().await.get_credential("anthropic").is_some(),
                "{name}"
            ),
        }
    }
}

#[tokio::test]
async fn set_api_key_fetches_that_providers_model_list() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let read = socket.read(&mut buf).await.unwrap();
        let body = r#"{"data":[{"id":"acme-1","context_length":65536}]}"#;
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(reply.as_bytes()).await.unwrap();
        String::from_utf8_lossy(&buf[..read]).to_lowercase()
    });

    let (mut client_reader, mut writer, temp_dir) = rpc_fixture("rpc_set_api_key_refresh");
    let mut active_turn = None;
    let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;
    ctx.live_models = true;
    ctx.config.write().await.providers.insert(
        "acme".to_string(),
        rho_harness_core::config::ProviderConfig {
            base_url: base,
            key_env: None,
            default_model: None,
        },
    );
    let catalog = ctx.engine.read().await.model_catalog().clone();
    let mut changes = catalog.subscribe();

    handle_set_api_key_cmd("acme".to_string(), "acme-key".to_string(), None, &mut ctx)
        .await
        .unwrap();
    assert!(read_response(&mut client_reader).await.success);

    let ready = async {
        loop {
            if let Some(rho_engine::provider::ProviderModels::Ready(models)) = catalog.get("acme") {
                break models;
            }
            changes.changed().await.unwrap();
        }
    };
    let models = tokio::time::timeout(std::time::Duration::from_secs(2), ready)
        .await
        .expect("acme's model list was not fetched after set_api_key");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "acme-1");
    assert_eq!(catalog.context_tokens("acme", "acme-1"), Some(65_536));
    let request = server.await.unwrap();
    assert!(request.starts_with("get /models "), "{request}");
    assert!(request.contains("authorization: bearer acme-key\r\n"), "{request}");
}

fn text(s: &str) -> RpcEvent {
    RpcEvent::TextChunk { content: s.to_string() }
}

fn reasoning(s: &str) -> RpcEvent {
    RpcEvent::ReasoningChunk { content: s.to_string() }
}

fn busy() -> RpcEvent {
    RpcEvent::StatusChanged {
        status: "busy".to_string(),
    }
}

#[tokio::test]
async fn a_token_stream_to_a_stalled_client_stays_one_queued_event() {
    let events = RpcEventQueue::default();
    let presenter = RpcPresenter::new(events.clone());

    for _ in 0..10_000 {
        presenter.print_token("tok ");
    }

    assert_eq!(events.next_batch().await, Some(vec![text(&"tok ".repeat(10_000))]));
}

#[tokio::test]
async fn queued_events_keep_their_order_with_adjacent_chunks_joined() {
    let events = RpcEventQueue::default();
    for event in [
        text("a"),
        text("b"),
        reasoning("c"),
        reasoning("d"),
        busy(),
        text("e"),
        text("f"),
    ] {
        events.push(event);
    }

    let batch = events.next_batch().await;

    assert_eq!(batch, Some(vec![text("ab"), reasoning("cd"), busy(), text("ef")]));
    events.close();
    assert_eq!(events.next_batch().await, None, "the batch took every queued event");
}

#[tokio::test]
async fn text_past_the_cap_drops_the_oldest_behind_one_marker() {
    let cap = 1024;
    let events = RpcEventQueue::with_text_byte_cap(cap);
    events.push(busy());
    let stream: String = (0..10_000).map(|i| format!("{}\u{20ac}", i % 10)).collect();
    for token in stream.split_inclusive('\u{20ac}') {
        events.push(text(token));
    }

    let batch = events.next_batch().await.unwrap();

    let [status, marker, RpcEvent::TextChunk { content }] = batch.as_slice() else {
        panic!("expected status, marker and text, got {batch:?}");
    };
    assert_eq!(*status, busy());
    assert_eq!(
        *marker,
        RpcEvent::OutputTruncated {
            dropped_bytes: stream.len() - content.len()
        }
    );
    assert!(content.len() <= cap, "{} buffered bytes", content.len());
    assert!(stream.ends_with(content.as_str()), "the newest text is kept");
}

#[tokio::test]
async fn text_dropped_after_a_later_event_is_reported_where_it_was_dropped() {
    let events = RpcEventQueue::with_text_byte_cap(8);
    let tool_start = RpcEvent::ToolCallStart {
        call_id: "call-1".to_string(),
        tool: "bash".to_string(),
        arguments: serde_json::json!({}),
    };
    events.push(text("aaaaaaaaaa"));
    events.push(tool_start.clone());

    events.push(text("bbbbbbb"));

    assert_eq!(
        events.next_batch().await,
        Some(vec![
            RpcEvent::OutputTruncated { dropped_bytes: 10 },
            tool_start,
            RpcEvent::OutputTruncated { dropped_bytes: 3 },
            text("bbbb"),
        ])
    );
}

#[tokio::test]
async fn a_closed_queue_refuses_events_after_yielding_the_queued_ones() {
    let events = RpcEventQueue::default();
    assert!(events.push(busy()));

    events.close();

    assert!(!events.push(text("late")));
    assert_eq!(events.next_batch().await, Some(vec![busy()]));
    assert_eq!(events.next_batch().await, None);
}

#[derive(Debug, Default)]
struct StalledAdapter {
    requested: tokio::sync::Notify,
    stream_alive: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

#[async_trait::async_trait]
impl rho_engine::provider::adapter::ModelAdapter for StalledAdapter {
    fn model_name(&self) -> &str {
        "stalled"
    }
    fn provider_name(&self) -> &str {
        "mock-provider"
    }
    async fn stream(
        &self,
        _request: rho_engine::provider::ModelCompletionRequest,
    ) -> rho_harness_core::error::Result<rho_engine::provider::adapter::ModelEventStream> {
        let alive = self.stream_alive.lock().unwrap().take();
        self.requested.notify_one();
        Ok(Box::pin(futures::stream::once(async move {
            let _alive = alive;
            std::future::pending().await
        })))
    }
}

/// Starts a turn whose model never answers; the receiver closes once the turn is dropped.
async fn start_stalled_turn<W: tokio::io::AsyncWrite + Unpin>(
    ctx: &mut RpcDaemonContext<'_, W>,
    dir: &std::path::Path,
) -> tokio::sync::oneshot::Receiver<()> {
    let (alive, dropped) = tokio::sync::oneshot::channel();
    let model = Arc::new(StalledAdapter {
        stream_alive: std::sync::Mutex::new(Some(alive)),
        ..StalledAdapter::default()
    });
    let config = Config {
        auth_file: dir.join("auth.json"),
        ..Config::default()
    };
    let engine = rho_engine::engine::eval::mock::mock_engine(
        model.clone(),
        rho_engine::engine::eval::mock::MockEngineConfig::new(dir, config),
    );
    ctx.engine = Arc::new(RwLock::new(engine));
    super::handlers::handle_prompt_cmd("go".to_string(), None, ctx)
        .await
        .unwrap();
    model.requested.notified().await;
    dropped
}

async fn turn_ended(dropped: tokio::sync::oneshot::Receiver<()>) -> bool {
    matches!(
        tokio::time::timeout(std::time::Duration::from_secs(1), dropped).await,
        Ok(Err(_))
    )
}

async fn run_summary_statuses<W>(ctx: &RpcDaemonContext<'_, W>) -> Vec<serde_json::Value> {
    ctx.engine
        .read()
        .await
        .session_manager
        .load_events()
        .await
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == rho_harness_core::session::SessionEventKind::RunSummary)
        .map(|event| event.payload["terminal_status"].clone())
        .collect()
}

#[tokio::test]
async fn exiting_during_a_turn_ends_it_as_cancelled_once() {
    let (_client, server_out) = duplex(4096);
    let mut writer = JsonLinesWriter::new(server_out);
    let temp_dir = rho_test_support::TestDir::new();
    let mut active_turn = None;
    let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;
    let turn_dropped = start_stalled_turn(&mut ctx, &temp_dir).await;

    let action = super::loop_runner::dispatch_rpc(RpcCommand::Exit, Some("exit-1".to_string()), &mut ctx)
        .await
        .unwrap();

    assert!(matches!(action, super::types::RpcLoopAction::Break));
    assert!(turn_ended(turn_dropped).await, "the turn is aborted");
    assert_eq!(run_summary_statuses(&ctx).await, vec![serde_json::json!("cancelled")]);
}

#[tokio::test(start_paused = true)]
async fn a_stop_is_read_while_events_wait_on_a_slow_client() {
    for command in [RpcCommand::Abort, RpcCommand::Exit] {
        let (_unread_client, server_out) = duplex(1024);
        let mut writer = JsonLinesWriter::new(server_out);
        let temp_dir = rho_test_support::TestDir::new();
        let mut active_turn = None;
        let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;
        let turn_dropped = start_stalled_turn(&mut ctx, &temp_dir).await;
        let (client_in, server_in) = duplex(4096);
        let mut reader = JsonLinesReader::new(tokio::io::BufReader::new(server_in));
        JsonLinesWriter::new(client_in)
            .write_message(&rho_harness_core::rpc::protocol::RpcRequest {
                id: Some("stop-1".to_string()),
                command: command.clone(),
            })
            .await
            .unwrap();
        let events = [RpcEvent::TextChunk {
            content: "x".repeat(4096),
        }];

        let write = super::loop_runner::write_events_while_reading(&mut reader, &mut ctx, &events);
        let pending = tokio::time::timeout(std::time::Duration::from_secs(1), write).await;

        assert!(
            pending.is_err(),
            "{command:?}: the client never reads, so the write stays pending"
        );
        assert!(
            turn_ended(turn_dropped).await,
            "{command:?}: the turn is aborted without waiting for the write"
        );
        assert_eq!(
            run_summary_statuses(&ctx).await,
            vec![serde_json::json!("cancelled")],
            "{command:?}"
        );
    }
}
