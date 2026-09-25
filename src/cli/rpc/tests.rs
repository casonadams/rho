use std::sync::Arc;
use tokio::io::duplex;
use tokio::sync::{RwLock, mpsc};

use super::handlers::config::{handle_config_update_cmd, handle_node_info_cmd, handle_state_command};
use super::handlers::session::{extract_chat_messages, handle_create_session_cmd, handle_get_tree_cmd};
use super::handlers::turn::{handle_tool_response_cmd, parse_tool_decision};
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

#[tokio::test]
async fn test_rpc_state_command_and_get_tree() {
    let (client_io, server_io) = duplex(4096);
    let mut client_reader = JsonLinesReader::new(tokio::io::BufReader::new(client_io));
    let mut writer = JsonLinesWriter::new(server_io);
    let temp_dir = std::env::temp_dir().join(format!("rpc_tree_test_{}", uuid::Uuid::new_v4()));
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

    let resp1: RpcResponse = client_reader.read_message().await.unwrap().unwrap();
    assert_eq!(resp1.id, Some("req-state".to_string()));
    let state_data = resp1.data.unwrap();
    assert!(state_data.get("active_workspace").is_some());
    assert!(state_data.get("total_input_tokens").is_some());
    assert!(state_data.get("context_window").is_some());

    let _resp2: RpcResponse = client_reader.read_message().await.unwrap().unwrap();
    let _resp3: RpcResponse = client_reader.read_message().await.unwrap().unwrap();

    let resp4: RpcResponse = client_reader.read_message().await.unwrap().unwrap();
    assert_eq!(resp4.id, Some("req-create".to_string()));
    let create_data = resp4.data.unwrap();
    assert!(create_data.get("active_workspace").is_some());
    assert!(create_data.get("total_input_tokens").is_some());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_rpc_config_update_handlers() {
    let (client_io, server_io) = duplex(4096);
    let mut client_reader = JsonLinesReader::new(tokio::io::BufReader::new(client_io));
    let mut writer = JsonLinesWriter::new(server_io);
    let temp_dir = std::env::temp_dir().join(format!("rpc_cfg_test_{}", uuid::Uuid::new_v4()));
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

    let resp_thinking: RpcResponse = client_reader.read_message().await.unwrap().unwrap();
    assert_eq!(resp_thinking.id, Some("req-thinking".to_string()));
    assert!(resp_thinking.success);

    let resp_model_ok: RpcResponse = client_reader.read_message().await.unwrap().unwrap();
    assert_eq!(resp_model_ok.id, Some("req-model-success".to_string()));
    assert!(resp_model_ok.success);

    let resp_model_err: RpcResponse = client_reader.read_message().await.unwrap().unwrap();
    assert_eq!(resp_model_err.id, Some("req-model-failure".to_string()));
    assert!(!resp_model_err.success);

    let resp_compact: RpcResponse = client_reader.read_message().await.unwrap().unwrap();
    assert_eq!(resp_compact.id, Some("req-compact".to_string()));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_extract_chat_messages_preserves_tools() {
    use rig::message::{
        AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
    };
    let msgs = vec![
        Message::user("run check"),
        Message::Assistant {
            id: None,
            content: vec![
                AssistantContent::text("I will run the command."),
                AssistantContent::ToolCall(ToolCall::new(
                    ToolCallId::new_or_mint("call_1"),
                    ToolFunction::new("bash".to_string(), serde_json::json!({ "command": "cargo check" })),
                )),
            ],
        },
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("call_1"),
                provider: None,
                name: "bash".to_string(),
                content: vec![ToolResultContent::Text(rig::message::Text::new(
                    "Finished dev [unoptimized + debuginfo]",
                ))],
            })],
        },
        Message::assistant("Check passed cleanly."),
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

#[tokio::test]
async fn test_handle_remote_auth_cmd_login_and_input() {
    use super::handlers::handle_remote_auth_cmd;
    use rho_harness_core::rpc::protocol::RpcCommand;

    let (client_io, server_io) = duplex(4096);
    let mut client_reader = JsonLinesReader::new(tokio::io::BufReader::new(client_io));
    let mut writer = JsonLinesWriter::new(server_io);
    let temp_dir = std::env::temp_dir().join(format!("rpc_auth_test_login_{}", uuid::Uuid::new_v4()));
    let mut active_turn = None;
    let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;

    let res = handle_remote_auth_cmd(
        &RpcCommand::AuthLogin {
            provider: "invalid_prov_xyz".to_string(),
        },
        Some("req-login-fail".to_string()),
        &mut ctx,
    )
    .await;
    assert!(res.unwrap());
    let resp = client_reader.read_message::<RpcResponse>().await.unwrap().unwrap();
    assert!(!resp.success);
    assert!(resp.error.unwrap().contains("Unknown provider"));

    let res = handle_remote_auth_cmd(
        &RpcCommand::AuthLogin {
            provider: "openai".to_string(),
        },
        Some("req-login-ok".to_string()),
        &mut ctx,
    )
    .await;
    assert!(res.unwrap());
    let resp = client_reader.read_message::<RpcResponse>().await.unwrap().unwrap();
    assert!(resp.success);

    let res = handle_remote_auth_cmd(
        &RpcCommand::AuthInput {
            interaction_id: "nonexistent".to_string(),
            secret_value: Some("sec".to_string()),
            selected_option: None,
        },
        Some("req-input".to_string()),
        &mut ctx,
    )
    .await;
    assert!(res.unwrap());
    let resp = client_reader.read_message::<RpcResponse>().await.unwrap().unwrap();
    assert!(resp.success);
    assert_eq!(resp.data.unwrap()["resolved"], false);

    let res = handle_remote_auth_cmd(&RpcCommand::GetState, Some("req-state".to_string()), &mut ctx).await;
    assert!(!res.unwrap());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_handle_remote_auth_cmd_keys_and_session() {
    use super::handlers::handle_remote_auth_cmd;
    use rho_harness_core::rpc::protocol::RpcCommand;

    let (client_io, server_io) = duplex(4096);
    let mut client_reader = JsonLinesReader::new(tokio::io::BufReader::new(client_io));
    let mut writer = JsonLinesWriter::new(server_io);
    let temp_dir = std::env::temp_dir().join(format!("rpc_auth_test_keys_{}", uuid::Uuid::new_v4()));
    let mut active_turn = None;
    let mut ctx = setup_test_rpc_context(&mut writer, &temp_dir, &mut active_turn).await;

    let res = handle_remote_auth_cmd(&RpcCommand::GetNodeInfo, Some("req-info".to_string()), &mut ctx).await;
    assert!(res.unwrap());
    let resp = client_reader.read_message::<RpcResponse>().await.unwrap().unwrap();
    assert!(resp.success);
    assert_eq!(resp.command, "get_node_info");

    let res = handle_remote_auth_cmd(
        &RpcCommand::CreateSession { workspace: None },
        Some("req-create".to_string()),
        &mut ctx,
    )
    .await;
    assert!(res.unwrap());
    let resp = client_reader.read_message::<RpcResponse>().await.unwrap().unwrap();
    assert!(resp.success);
    assert_eq!(resp.command, "create_session");

    let res = handle_remote_auth_cmd(
        &RpcCommand::SetApiKey {
            provider: "anthropic".to_string(),
            api_key: "sk-ant-testkey".to_string(),
        },
        Some("req-key-ok".to_string()),
        &mut ctx,
    )
    .await;
    assert!(res.unwrap());
    let resp = client_reader.read_message::<RpcResponse>().await.unwrap().unwrap();
    assert!(resp.success);
    assert!(ctx.auth_store.read().await.get_credential("anthropic").is_some());

    let _ = std::fs::remove_dir_all(&temp_dir);
}
