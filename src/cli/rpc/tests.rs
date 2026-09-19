use std::sync::Arc;
use tokio::io::duplex;
use tokio::sync::{RwLock, mpsc};

use super::handlers::config::{handle_node_info_cmd, handle_state_command};
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
use rho_harness_core::rpc::protocol::{RpcEvent, RpcResponse};
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

#[tokio::test]
async fn test_rpc_state_command_and_get_tree() {
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let presenter = RpcPresenter::new(event_tx);
    let pending = presenter.pending_approvals();

    let (client_io, server_io) = duplex(4096);
    let mut client_reader = JsonLinesReader::new(tokio::io::BufReader::new(client_io));
    let mut writer = JsonLinesWriter::new(server_io);
    let temp_dir = std::env::temp_dir().join(format!("rpc_tree_test_{}", uuid::Uuid::new_v4()));
    let config = Config {
        sessions_dir: temp_dir.join("sessions"),
        auth_file: temp_dir.join("auth.json"),
        model: "mock-model".to_string(),
        provider: "ollama".to_string(),
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
