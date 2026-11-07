use std::sync::Arc;
use tokio::io::duplex;
use tokio::sync::{RwLock, mpsc};

use super::handlers::config::{handle_config_update_cmd, handle_node_info_cmd, handle_state_command};
use super::handlers::session::{extract_chat_messages, handle_create_session_cmd, handle_get_tree_cmd};
use super::handlers::turn::{handle_tool_response_cmd, parse_tool_decision};
use super::handlers::{handle_node_session_auth_cmd, handle_resume_or_fork_cmd};
use super::types::RpcDaemonContext;
use crate::auth::AuthStore;
use crate::config::Config;
use crate::repl::coordinator::SharedSteeringQueue;
use crate::ui::render::RpcPresenter;
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

#[tokio::test]
async fn test_rpc_presenter_tool_approval_roundtrip() {
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let presenter = RpcPresenter::new(event_tx);
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

    // 1. First event: ToolApprovalRequest
    let ev1 = event_rx.recv().await.expect("expected tool approval event");
    let approval_id = match ev1 {
        RpcEvent::ToolApprovalRequest { approval_id, tool, .. } => {
            assert_eq!(tool, "bash");
            approval_id
        }
        other => panic!("expected ToolApprovalRequest, got {other:?}"),
    };

    // 2. Second event: StatusChanged { status: "waiting_approval" }
    let ev2 = event_rx.recv().await.expect("expected status change");
    assert_eq!(
        ev2,
        RpcEvent::StatusChanged {
            status: "waiting_approval".to_string()
        }
    );

    // 3. Resolve approval via handle_tool_response_cmd
    let (_client_io, server_io) = duplex(1024);
    let mut writer = JsonLinesWriter::new(server_io);
    let temp_dir = std::env::temp_dir().join(format!("rpc_test_{}", uuid::Uuid::new_v4()));
    let config = Config {
        sessions_dir: temp_dir.join("sessions"),
        auth_file: temp_dir.join("auth.json"),
        ..Config::default()
    };
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), None)
        .await
        .unwrap();
    let engine_lock = Arc::new(RwLock::new(engine));
    let config_lock = Arc::new(RwLock::new(config));
    let auth_store_lock = Arc::new(RwLock::new(auth_store));
    let steering = Arc::new(SharedSteeringQueue::new(rho_engine::engine::runner::QueueMode::All));
    let mut active_turn = None;

    let (test_event_tx, _test_event_rx) = mpsc::unbounded_channel();
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
        event_tx: test_event_tx,
        auth_bridge: rho_harness_core::rpc::RpcAuthBridge::new(),
    };

    handle_tool_response_cmd((approval_id, "allow".to_string(), None), &mut ctx)
        .await
        .unwrap();

    let response = handle.await.unwrap();
    assert_eq!(response, Some(InteractionResponse::Selected(0)));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

async fn setup_test_rpc_context<'a, W: tokio::io::AsyncWrite + Unpin>(
    writer: &'a mut JsonLinesWriter<W>,
    temp_dir: &std::path::Path,
    active_turn: &'a mut Option<tokio::task::JoinHandle<crate::error::Result<rho_engine::engine::runner::TurnOutput>>>,
) -> RpcDaemonContext<'a, W> {
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let presenter = RpcPresenter::new(event_tx);
    let pending = presenter.pending_approvals();
    let config = Config {
        sessions_dir: temp_dir.join("sessions"),
        auth_file: temp_dir.join("auth.json"),
        model: "mock-model".to_string(),
        provider: "local".to_string(),
        ..Config::default()
    };
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), None)
        .await
        .unwrap();
    let (test_event_tx, _test_event_rx) = mpsc::unbounded_channel();
    RpcDaemonContext {
        writer,
        engine: Arc::new(RwLock::new(engine)),
        presenter: Arc::new(presenter),
        config: Arc::new(RwLock::new(config)),
        auth_store: Arc::new(RwLock::new(auth_store)),
        pending_approvals: pending,
        steering: Arc::new(SharedSteeringQueue::new(rho_engine::engine::runner::QueueMode::All)),
        active_turn,
        event_tx: test_event_tx,
        auth_bridge: rho_harness_core::rpc::RpcAuthBridge::new(),
    }
}

type ClientReader = JsonLinesReader<tokio::io::BufReader<tokio::io::DuplexStream>>;

fn rpc_fixture(
    prefix: &str,
) -> (
    ClientReader,
    JsonLinesWriter<tokio::io::DuplexStream>,
    std::path::PathBuf,
) {
    let (client_io, server_io) = duplex(4096);
    let temp_dir = std::env::temp_dir().join(format!("{prefix}_{}", uuid::Uuid::new_v4()));
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

    let _resp2 = read_response(&mut client_reader).await;
    let _resp3 = read_response(&mut client_reader).await;

    let resp4 = read_response(&mut client_reader).await;
    assert_eq!(resp4.id, Some("req-create".to_string()));
    let create_data = resp4.data.unwrap();
    assert!(create_data.get("active_workspace").is_some());
    assert!(create_data.get("total_input_tokens").is_some());

    let _ = std::fs::remove_dir_all(&temp_dir);
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

    let resp_model_ok = read_response(&mut client_reader).await;
    assert_eq!(resp_model_ok.id, Some("req-model-success".to_string()));
    assert!(resp_model_ok.success);

    let resp_model_err = read_response(&mut client_reader).await;
    assert_eq!(resp_model_err.id, Some("req-model-failure".to_string()));
    assert!(!resp_model_err.success);

    let resp_compact = read_response(&mut client_reader).await;
    assert_eq!(resp_compact.id, Some("req-compact".to_string()));

    let _ = std::fs::remove_dir_all(&temp_dir);
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
    let engine = crate::platform::agent_engine(config.clone(), auth_store.clone(), None)
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

    let temp_dir = std::env::temp_dir().join(format!("rpc_stream_test_{}", uuid::Uuid::new_v4()));
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
    let _ = std::fs::remove_dir_all(&temp_dir);
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

    let _ = std::fs::remove_dir_all(&temp_dir);
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
        (
            RpcCommand::SwitchBranch {
                node_id: "nonexistent-node-xyz".to_string(),
            },
            "req-switch",
            "switch_branch",
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

    let _ = std::fs::remove_dir_all(&temp_dir);
}
