use std::net::SocketAddr;
use std::sync::Arc;

use iroh::Endpoint;
use iroh::endpoint::presets::Minimal;
use rho_harness_core::collab::{
    COLLAB_ALPN, CapabilityLevel, CollabHostConfig, CollabHostServer, CollabPeerEvent, CollabSessionStream,
    CollabSnapshot,
};
use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent};

#[tokio::test]
async fn test_collab_copilot_full_lifecycle() {
    let initial_snapshot =
        CollabSnapshot::new(vec![serde_json::json!({"turn": 1, "prompt": "initial prompt"})], "idle");
    let config = CollabHostConfig::new()
        .with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
        .with_snapshot_provider({
            let snap = initial_snapshot.clone();
            move || snap.clone()
        });

    let server = Arc::new(CollabHostServer::start(config).await.expect("host server"));
    let (full_ticket, _) = server.tickets().await.expect("tickets");

    let guest_ep = Endpoint::builder(Minimal)
        .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
        .expect("bind addr")
        .bind()
        .await
        .expect("guest endpoint");

    let conn = guest_ep
        .connect(server.endpoint().addr(), COLLAB_ALPN)
        .await
        .expect("connect");
    let (send, recv) = conn.accept_bi().await.expect("accept_bi");

    let mut guest_stream = CollabSessionStream::connect_guest_with_ticket(send, recv, &full_ticket)
        .await
        .expect("connect guest");
    assert_eq!(guest_stream.role(), CapabilityLevel::Full);

    // Verify snapshot received on guest
    let snap = guest_stream.recv_snapshot().await.expect("recv snapshot");
    assert_eq!(snap, Some(initial_snapshot));

    // Verify peer event on host
    let peer_event = server.recv_peer_event().await;
    assert!(
        matches!(peer_event, Some(rho_harness_core::collab::CollabPeerEvent::Connected(info)) if info.id == 1 && info.role == CapabilityLevel::Full)
    );
    assert_eq!(server.peer_count().await, 1);

    // Host broadcasts event, guest receives it
    let test_event = RpcEvent::TextChunk {
        content: "streaming token delta".to_string(),
    };
    server.broadcast(&test_event);
    let received_event = guest_stream.recv_event().await.expect("recv event");
    assert_eq!(received_event, Some(test_event));

    // Guest sends a command, host receives it
    guest_stream
        .send_command(&RpcCommand::Prompt {
            message: "implement feature".to_string(),
            images: None,
            streaming_behavior: None,
        })
        .await
        .expect("send command");

    let host_cmd = server.recv_command().await.expect("recv command");
    assert_eq!(host_cmd.peer_id, 1);
    assert_eq!(host_cmd.role, CapabilityLevel::Full);
    assert!(matches!(host_cmd.command, RpcCommand::Prompt { message, .. } if message == "implement feature"));

    server.stop().await;
    guest_ep.close().await;
}

#[tokio::test]
async fn test_collab_view_only_rejection() {
    let config = CollabHostConfig::new().with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)));
    let server = Arc::new(CollabHostServer::start(config).await.expect("host server"));
    let (_, view_ticket) = server.tickets().await.expect("tickets");

    let guest_ep = Endpoint::builder(Minimal)
        .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
        .expect("bind addr")
        .bind()
        .await
        .expect("guest endpoint");

    let conn = guest_ep
        .connect(server.endpoint().addr(), COLLAB_ALPN)
        .await
        .expect("connect");
    let (send, recv) = conn.accept_bi().await.expect("accept_bi");

    let mut guest_stream = CollabSessionStream::connect_guest_with_ticket(send, recv, &view_ticket)
        .await
        .expect("connect guest");
    assert_eq!(guest_stream.role(), CapabilityLevel::ViewOnly);

    let _snap = guest_stream.recv_snapshot().await.expect("recv snapshot");

    // 1. Client-side rejection via send_command
    let client_res = guest_stream
        .send_command(&RpcCommand::Prompt {
            message: "malicious prompt".to_string(),
            images: None,
            streaming_behavior: None,
        })
        .await;
    assert!(client_res.is_err(), "expected client-side rejection");

    // 2. Host-side wire rejection via raw message
    guest_stream
        .writer
        .send_raw(&RpcCommand::Prompt {
            message: "bypassed client check".to_string(),
            images: None,
            streaming_behavior: None,
        })
        .await
        .expect("send raw");
    guest_stream.writer.flush().await.expect("flush");

    // Host should receive raw command and respond with UNAUTHORIZED error
    let err_ev = guest_stream.recv_event().await.expect("recv event");
    assert!(
        matches!(err_ev, Some(RpcEvent::Error { ref code, .. }) if code == "UNAUTHORIZED"),
        "expected UNAUTHORIZED error, got {err_ev:?}"
    );

    server.stop().await;
    guest_ep.close().await;
}

#[tokio::test]
async fn test_collab_shared_tool_approval() {
    let config = CollabHostConfig::new().with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)));
    let server = Arc::new(CollabHostServer::start(config).await.expect("host server"));
    let (full_ticket, _) = server.tickets().await.expect("tickets");

    let guest_ep = Endpoint::builder(Minimal)
        .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
        .expect("bind addr")
        .bind()
        .await
        .expect("guest endpoint");

    let conn = guest_ep
        .connect(server.endpoint().addr(), COLLAB_ALPN)
        .await
        .expect("connect");
    let (send, recv) = conn.accept_bi().await.expect("accept_bi");

    let mut guest_stream = CollabSessionStream::connect_guest_with_ticket(send, recv, &full_ticket)
        .await
        .expect("connect guest");
    let _ = guest_stream.recv_snapshot().await.expect("recv snapshot");
    let _ = server.recv_peer_event().await.expect("peer event");

    let (ui, _events) = rho::ui::interactive::InteractiveUi::channel();
    let term_renderer = Arc::new(rho::ui::render::TerminalRenderer::with_ui(ui));
    let collab_presenter = Arc::new(rho::repl::CollabPresenter::new(term_renderer, Arc::clone(&server)));

    let cp_clone = Arc::clone(&collab_presenter);
    let prompt = rho_harness_core::presentation::InteractionPrompt {
        title: "bash".into(),
        body: "Run: rm -rf /tmp/test".into(),
        options: vec![],
        allow_custom: false,
        option_layout: rho_harness_core::presentation::OptionLayout::Vertical,
        initial_selection: 0,
        initial_text: None,
    };
    let host_task = tokio::spawn(async move {
        use rho_harness_core::presentation::presenter::Presenter;
        cp_clone.request_interaction(prompt).await
    });

    let approval_event = guest_stream.recv_event().await.expect("recv approval event");
    let approval_id = match approval_event {
        Some(RpcEvent::ToolApprovalRequest { approval_id, tool, .. }) => {
            assert_eq!(tool, "bash");
            approval_id
        }
        other => panic!("expected ToolApprovalRequest, got {other:?}"),
    };

    guest_stream
        .send_command(&RpcCommand::ToolResponse {
            approval_id: approval_id.clone(),
            decision: "allow".into(),
        })
        .await
        .expect("send tool response");

    let incoming = server.recv_command().await.expect("recv command");
    if let RpcCommand::ToolResponse {
        approval_id: id,
        decision,
    } = incoming.command
    {
        assert_eq!(id, approval_id);
        let resolved = collab_presenter.resolve_tool_response(&id, &decision).await;
        assert!(resolved);
    }

    let host_res = host_task.await.expect("host task finished");
    assert_eq!(
        host_res,
        Some(rho_harness_core::presentation::InteractionResponse::Selected(0))
    );

    let resolved_event = guest_stream.recv_event().await.expect("recv resolved event");
    assert!(
        matches!(resolved_event, Some(RpcEvent::ToolApprovalResolved { approval_id: ref id, ref decision }) if id == &approval_id && decision.as_deref() == Some("allow")),
        "expected ToolApprovalResolved, got {resolved_event:?}"
    );

    server.stop().await;
    guest_ep.close().await;
}

struct IntegrationMockBackend;
impl rho::ui::interactive::TerminalBackend for IntegrationMockBackend {
    fn set_raw_mode(&mut self, _: bool) -> std::io::Result<()> {
        Ok(())
    }
    fn size(&self) -> std::io::Result<(u16, u16)> {
        Ok((80, 24))
    }
    fn hide_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn show_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn move_up(&mut self, _: usize) -> std::io::Result<()> {
        Ok(())
    }
    fn move_down(&mut self, _: usize) -> std::io::Result<()> {
        Ok(())
    }
    fn move_to_column(&mut self, _: usize) -> std::io::Result<()> {
        Ok(())
    }
    fn clear_line(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn write_text(&mut self, _: &str) -> std::io::Result<()> {
        Ok(())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct TestCollabFixture {
    server: Arc<CollabHostServer>,
    guest_ep: Endpoint,
    guest_writer: rho_harness_core::collab::session::CollabWriter<iroh::endpoint::SendStream>,
    guest_reader: rho_harness_core::collab::session::CollabReader<iroh::endpoint::RecvStream>,
    snapshot: CollabSnapshot,
}

async fn start_test_collab_fixture(snapshot: CollabSnapshot) -> TestCollabFixture {
    start_test_collab_fixture_with_hostname(snapshot, None).await
}

async fn start_test_collab_fixture_with_hostname(
    snapshot: CollabSnapshot,
    hostname: Option<String>,
) -> TestCollabFixture {
    let config = CollabHostConfig::new()
        .with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
        .with_snapshot_provider({
            let snap = snapshot.clone();
            move || snap.clone()
        });

    let server = Arc::new(CollabHostServer::start(config).await.expect("host server"));
    let (full_ticket, _) = server.tickets().await.expect("tickets");

    let guest_ep = Endpoint::builder(Minimal)
        .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
        .expect("bind addr")
        .bind()
        .await
        .expect("guest endpoint");

    let conn = guest_ep
        .connect(server.endpoint().addr(), COLLAB_ALPN)
        .await
        .expect("connect");
    let (send, recv) = conn.accept_bi().await.expect("accept_bi");

    let mut guest_stream =
        CollabSessionStream::connect_guest_with_ticket_and_hostname(send, recv, &full_ticket, hostname)
            .await
            .expect("connect guest");

    let snap = guest_stream
        .recv_snapshot()
        .await
        .expect("recv snapshot")
        .expect("some snapshot");

    let (guest_writer, guest_reader) = guest_stream.into_split();
    TestCollabFixture {
        server,
        guest_ep,
        guest_writer,
        guest_reader,
        snapshot: snap,
    }
}

#[tokio::test]
async fn test_collab_guest_interactive_tui_pipeline() {
    let turn = rho_harness_core::session::turns::ConversationTurn {
        turn_number: 1,
        user_prompt: "prior user message".into(),
        assistant_preview: "prior assistant message".into(),
        tool_calls_count: 0,
    };
    let initial_snapshot = CollabSnapshot::new(vec![serde_json::to_value(turn).expect("turn json")], "idle");
    let mut fixture = start_test_collab_fixture(initial_snapshot.clone()).await;
    assert_eq!(fixture.snapshot, initial_snapshot);

    let (ui, _ui_events) = rho::ui::interactive::InteractiveUi::channel();
    let renderer = rho::ui::TerminalRenderer::with_ui(ui);

    let mut state = rho::ui::interactive::InteractiveState::default();
    rho::cli::collab::guest::configure_guest_state(&mut state, CapabilityLevel::Full);
    assert_eq!(state.footer().model, "co-pilot");

    let mut controller =
        rho::ui::interactive::TerminalController::new(IntegrationMockBackend, state).expect("controller");
    rho::cli::collab::guest::hydrate_snapshot_controller(&mut controller, &fixture.snapshot).expect("hydrate snapshot");
    assert_eq!(controller.transcript().len(), 2);

    let mut approvals = rho::cli::collab::guest::GuestApprovalState::default();

    fixture.server.broadcast(&RpcEvent::TextChunk {
        content: "streamed token".to_string(),
    });
    let ev = fixture
        .guest_reader
        .recv_event()
        .await
        .expect("recv")
        .expect("some event");
    let ok = rho::cli::collab::guest::handle_guest_rpc_event(&ev, &renderer, &mut controller, &mut approvals)
        .await
        .expect("handle event");
    assert!(ok);

    fixture.server.broadcast(&RpcEvent::ToolApprovalRequest {
        approval_id: "req-1".to_string(),
        tool: "bash".to_string(),
        arguments: serde_json::json!({"command": "cargo check"}),
        description: None,
    });
    let ev_app = fixture
        .guest_reader
        .recv_event()
        .await
        .expect("recv")
        .expect("some event");
    rho::cli::collab::guest::handle_guest_rpc_event(&ev_app, &renderer, &mut controller, &mut approvals)
        .await
        .expect("handle approval");
    assert_eq!(approvals.current_id.as_deref(), Some("req-1"));
    assert!(controller.state().active_modal().is_some());

    fixture.server.broadcast(&RpcEvent::ToolApprovalResolved {
        approval_id: "req-1".to_string(),
        decision: Some("allow".to_string()),
    });
    let ev_res = fixture
        .guest_reader
        .recv_event()
        .await
        .expect("recv")
        .expect("some event");
    rho::cli::collab::guest::handle_guest_rpc_event(&ev_res, &renderer, &mut controller, &mut approvals)
        .await
        .expect("handle resolved");
    assert!(controller.state().active_modal().is_none());
    assert_eq!(approvals.current_id, None);

    drop(fixture.guest_writer);
    fixture.server.stop().await;
    fixture.guest_ep.close().await;
}

#[tokio::test]
async fn test_collab_prompt_command_turn_start_sync() {
    let config = CollabHostConfig::new().with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)));
    let server = Arc::new(CollabHostServer::start(config).await.expect("host server"));
    let (full_ticket, _) = server.tickets().await.expect("tickets");

    let guest_ep = Endpoint::builder(Minimal)
        .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
        .expect("bind addr")
        .bind()
        .await
        .expect("guest endpoint");

    let conn = guest_ep
        .connect(server.endpoint().addr(), COLLAB_ALPN)
        .await
        .expect("connect");
    let (send, recv) = conn.accept_bi().await.expect("accept_bi");

    let mut guest_stream = CollabSessionStream::connect_guest_with_ticket_and_hostname(
        send,
        recv,
        &full_ticket,
        Some("copilot-m3".into()),
    )
    .await
    .expect("connect guest");

    let _snap = guest_stream.recv_snapshot().await.expect("recv snapshot");
    let _peer_ev = server.recv_peer_event().await.expect("recv peer event");

    guest_stream
        .send_command(&RpcCommand::Prompt {
            message: "refactor module".into(),
            images: None,
            streaming_behavior: None,
        })
        .await
        .expect("send prompt command");

    let host_cmd = server.recv_command().await.expect("recv command");
    assert_eq!(host_cmd.peer_id, 1);
    assert_eq!(
        server.peer_display_name_sync(host_cmd.peer_id).as_deref(),
        Some("copilot-m3")
    );

    let turn_start = RpcEvent::TurnStart {
        turn_number: 2,
        prompt: "refactor module".into(),
    };
    server.broadcast(&turn_start);

    let received_event = guest_stream.recv_event().await.expect("recv event");
    assert_eq!(received_event, Some(turn_start));

    server.stop().await;
    guest_ep.close().await;
}

#[tokio::test]
async fn test_collab_footer_metric_and_activity_sync() {
    let initial_snapshot = CollabSnapshot::new(vec![], "idle");
    let mut fixture = start_test_collab_fixture(initial_snapshot).await;
    let (ui, _ui_rx) = rho::ui::interactive::InteractiveUi::channel();
    let renderer = rho::ui::TerminalRenderer::with_ui(ui);
    let mut state = rho::ui::interactive::InteractiveState::default();
    rho::cli::collab::guest::configure_guest_state(&mut state, CapabilityLevel::Full);
    let mut controller =
        rho::ui::interactive::TerminalController::new(IntegrationMockBackend, state).expect("controller");
    let mut approvals = rho::cli::collab::guest::GuestApprovalState::default();

    assert_eq!(
        controller.state().footer().activity,
        rho::ui::interactive::Activity::Idle
    );
    assert_eq!(controller.state().footer().provider, "collab");

    let session_start = RpcEvent::SessionStart {
        session_id: "test-sess".into(),
        model: "claude-3-7-sonnet".into(),
        provider: "anthropic".into(),
    };
    fixture.server.broadcast(&session_start);
    let ev = fixture.guest_reader.recv_event().await.expect("recv").expect("some");
    rho::cli::collab::guest::handle_guest_rpc_event(&ev, &renderer, &mut controller, &mut approvals)
        .await
        .expect("handle session start");
    assert_eq!(controller.state().footer().model, "claude-3-7-sonnet");
    assert_eq!(controller.state().footer().provider, "anthropic");

    let turn_start = RpcEvent::TurnStart {
        turn_number: 1,
        prompt: "hello world".into(),
    };
    fixture.server.broadcast(&turn_start);
    let ev = fixture.guest_reader.recv_event().await.expect("recv").expect("some");
    rho::cli::collab::guest::handle_guest_rpc_event(&ev, &renderer, &mut controller, &mut approvals)
        .await
        .expect("handle turn start");
    assert_eq!(
        controller.state().footer().activity,
        rho::ui::interactive::Activity::Working
    );

    let usage = RpcEvent::UsageUpdate {
        input_tokens: Some(500),
        output_tokens: Some(120),
        cache_read_tokens: Some(50),
        cache_write_tokens: Some(20),
        total_cost: Some(0.015),
        context_percent: Some(18.5),
        context_window: Some(200_000),
        tokens_per_second: Some(42.0),
        quota: Some("75%".into()),
    };
    fixture.server.broadcast(&usage);
    let ev = fixture.guest_reader.recv_event().await.expect("recv").expect("some");
    rho::cli::collab::guest::handle_guest_rpc_event(&ev, &renderer, &mut controller, &mut approvals)
        .await
        .expect("handle usage update");
    let f = controller.state().footer();
    assert_eq!(f.total_input_tokens, 500);
    assert_eq!(f.total_output_tokens, 120);
    assert_eq!(f.total_cache_read_tokens, 50);
    assert_eq!(f.total_cache_write_tokens, 20);
    assert_eq!(f.total_cost, Some(0.015));
    assert_eq!(f.context_percent, Some(18.5));
    assert_eq!(f.context_window, 200_000);
    assert_eq!(f.tokens_per_second, Some(42.0));
    assert_eq!(f.quota, Some("75%".into()));

    let turn_end = RpcEvent::TurnEnd {
        stop_reason: "completed".into(),
    };
    fixture.server.broadcast(&turn_end);
    let ev = fixture.guest_reader.recv_event().await.expect("recv").expect("some");
    rho::cli::collab::guest::handle_guest_rpc_event(&ev, &renderer, &mut controller, &mut approvals)
        .await
        .expect("handle turn end");
    assert_eq!(
        controller.state().footer().activity,
        rho::ui::interactive::Activity::Idle
    );

    drop(fixture.guest_writer);
    fixture.server.stop().await;
    fixture.guest_ep.close().await;
}

fn make_test_host_session(
    server: &Arc<CollabHostServer>,
    renderer: rho::ui::TerminalRenderer,
) -> rho::repl::ReplSession {
    let mut session =
        rho::repl::ReplSession::new(rho::config::Config::default(), rho::auth::AuthStore::default(), None);
    session.renderer = renderer;
    session.collab = Some(Arc::clone(server));
    session.config.model = "claude-3-7-sonnet".to_string();
    session.config.provider = "anthropic".to_string();
    session
}

fn make_test_guest_controller() -> rho::ui::interactive::TerminalController<IntegrationMockBackend> {
    let mut state = rho::ui::interactive::InteractiveState::default();
    rho::cli::collab::guest::configure_guest_state(&mut state, CapabilityLevel::Full);
    rho::ui::interactive::TerminalController::new(IntegrationMockBackend, state).expect("controller")
}

async fn assert_host_transcript_notice(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<rho::ui::interactive::UiEvent>,
    expected: &str,
) {
    let notice = rx.recv().await.expect("host notice");
    match notice {
        rho::ui::interactive::UiEvent::Transcript(rho::ui::interactive::TranscriptItem::Notice(text)) => {
            assert!(text.contains(expected));
        }
        _ => panic!("expected TranscriptItem::Notice"),
    }
}

async fn assert_guest_user_message(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<rho::ui::interactive::UiEvent>,
    expected: &str,
) {
    let ev = rx.recv().await.expect("guest user message");
    match ev {
        rho::ui::interactive::UiEvent::Transcript(rho::ui::interactive::TranscriptItem::UserMessage(prompt)) => {
            assert_eq!(prompt, expected);
        }
        _ => panic!("expected TranscriptItem::UserMessage"),
    }
}

async fn handle_next_guest_event(
    reader: &mut rho_harness_core::collab::session::CollabReader<iroh::endpoint::RecvStream>,
    renderer: &rho::ui::TerminalRenderer,
    controller: &mut rho::ui::interactive::TerminalController<IntegrationMockBackend>,
    approvals: &mut rho::cli::collab::guest::GuestApprovalState,
) {
    let ev = reader.recv_event().await.expect("recv").expect("event");
    rho::cli::collab::guest::handle_guest_rpc_event(&ev, renderer, controller, approvals)
        .await
        .expect("handle rpc event");
}

#[tokio::test]
async fn test_collab_lifecycle_peer_connect_and_prompt_sync() {
    let initial_snapshot = CollabSnapshot::new(vec![], "idle");
    let mut fixture = start_test_collab_fixture_with_hostname(initial_snapshot, Some("cadams-laptop".into())).await;

    let (host_ui, mut host_ui_rx) = rho::ui::interactive::InteractiveUi::channel();
    let host_session = make_test_host_session(&fixture.server, rho::ui::TerminalRenderer::with_ui(host_ui));

    let (guest_ui, mut guest_ui_rx) = rho::ui::interactive::InteractiveUi::channel();
    let guest_renderer = rho::ui::TerminalRenderer::with_ui(guest_ui);
    let mut guest_controller = make_test_guest_controller();
    let mut guest_approvals = rho::cli::collab::guest::GuestApprovalState::default();

    let peer_event = fixture.server.recv_peer_event().await.expect("peer event");
    assert!(
        matches!(peer_event, CollabPeerEvent::Connected(ref info) if info.hostname.as_deref() == Some("cadams-laptop"))
    );
    rho::repl::handle_collab_peer_event(&peer_event, &host_session, None, 1);
    assert_host_transcript_notice(
        &mut host_ui_rx,
        "Collaborator connected: cadams-laptop (co-pilot, 1 peer(s) total)",
    )
    .await;

    handle_next_guest_event(
        &mut fixture.guest_reader,
        &guest_renderer,
        &mut guest_controller,
        &mut guest_approvals,
    )
    .await;
    assert_eq!(guest_controller.state().footer().model, "claude-3-7-sonnet");
    assert_eq!(guest_controller.state().footer().provider, "anthropic");

    let prompt_cmd = RpcCommand::Prompt {
        message: "refactor auth module".into(),
        images: None,
        streaming_behavior: None,
    };
    fixture
        .guest_writer
        .send_command(&prompt_cmd)
        .await
        .expect("send prompt");

    let received_cmd = fixture.server.recv_command().await.expect("server recv command");
    assert_eq!(received_cmd.peer_id, 1);
    let queued = rho::repl::handle_collab_idle_command(received_cmd, &host_session);
    assert!(matches!(queued, Some(ref q) if q.text == "refactor auth module"));
    assert_host_transcript_notice(
        &mut host_ui_rx,
        "Collaborator prompt [cadams-laptop]: refactor auth module",
    )
    .await;

    let turn_start_ev = RpcEvent::TurnStart {
        turn_number: 1,
        prompt: "refactor auth module".into(),
    };
    fixture.server.broadcast(&turn_start_ev);

    handle_next_guest_event(
        &mut fixture.guest_reader,
        &guest_renderer,
        &mut guest_controller,
        &mut guest_approvals,
    )
    .await;
    assert_eq!(
        guest_controller.state().footer().activity,
        rho::ui::interactive::Activity::Working
    );

    assert_guest_user_message(&mut guest_ui_rx, "refactor auth module").await;

    fixture.server.stop().await;
    fixture.guest_ep.close().await;
}

#[tokio::test]
async fn test_collab_lifecycle_streaming_usage_and_turn_completion() {
    let initial_snapshot = CollabSnapshot::new(vec![], "idle");
    let mut fixture = start_test_collab_fixture_with_hostname(initial_snapshot, Some("cadams-laptop".into())).await;

    let (host_ui, mut host_ui_rx) = rho::ui::interactive::InteractiveUi::channel();
    let host_session = make_test_host_session(&fixture.server, rho::ui::TerminalRenderer::with_ui(host_ui));
    let _connect_event = fixture.server.recv_peer_event().await.expect("connect event");

    let (guest_ui, _guest_ui_rx) = rho::ui::interactive::InteractiveUi::channel();
    let guest_renderer = rho::ui::TerminalRenderer::with_ui(guest_ui);
    let mut guest_controller = make_test_guest_controller();
    let mut guest_approvals = rho::cli::collab::guest::GuestApprovalState::default();

    let usage_ev = RpcEvent::UsageUpdate {
        input_tokens: Some(1250),
        output_tokens: Some(340),
        cache_read_tokens: Some(150),
        cache_write_tokens: Some(60),
        total_cost: Some(0.024),
        context_percent: Some(22.0),
        context_window: Some(200_000),
        tokens_per_second: Some(48.5),
        quota: Some("80%".into()),
    };
    fixture.server.broadcast(&usage_ev);
    handle_next_guest_event(
        &mut fixture.guest_reader,
        &guest_renderer,
        &mut guest_controller,
        &mut guest_approvals,
    )
    .await;
    let f = guest_controller.state().footer();
    assert_eq!(f.total_input_tokens, 1250);
    assert_eq!(f.total_output_tokens, 340);
    assert_eq!(f.total_cache_read_tokens, 150);
    assert_eq!(f.total_cache_write_tokens, 60);
    assert_eq!(f.total_cost, Some(0.024));
    assert_eq!(f.context_percent, Some(22.0));
    assert_eq!(f.tokens_per_second, Some(48.5));
    assert_eq!(f.quota, Some("80%".into()));

    let turn_end_ev = RpcEvent::TurnEnd {
        stop_reason: "completed".into(),
    };
    fixture.server.broadcast(&turn_end_ev);
    handle_next_guest_event(
        &mut fixture.guest_reader,
        &guest_renderer,
        &mut guest_controller,
        &mut guest_approvals,
    )
    .await;
    assert_eq!(
        guest_controller.state().footer().activity,
        rho::ui::interactive::Activity::Idle
    );

    drop(fixture.guest_writer);
    let disconnect_event = fixture.server.recv_peer_event().await.expect("disconnect event");
    assert!(
        matches!(disconnect_event, CollabPeerEvent::Disconnected { ref display_name, .. } if display_name == "cadams-laptop")
    );
    rho::repl::handle_collab_peer_event(&disconnect_event, &host_session, None, 0);
    assert_host_transcript_notice(
        &mut host_ui_rx,
        "Collaborator cadams-laptop disconnected (0 peer(s) remaining)",
    )
    .await;

    fixture.server.stop().await;
    fixture.guest_ep.close().await;
}

#[tokio::test]
async fn test_collab_peer_hostname_fallback_when_none() {
    let initial_snapshot = CollabSnapshot::new(vec![], "idle");
    let fixture = start_test_collab_fixture_with_hostname(initial_snapshot, None).await;

    let (host_ui, mut host_ui_rx) = rho::ui::interactive::InteractiveUi::channel();
    let host_session = make_test_host_session(&fixture.server, rho::ui::TerminalRenderer::with_ui(host_ui));

    let peer_event = fixture.server.recv_peer_event().await.expect("peer event");
    assert!(matches!(peer_event, CollabPeerEvent::Connected(ref info) if info.hostname.is_none()));
    rho::repl::handle_collab_peer_event(&peer_event, &host_session, None, 1);
    assert_host_transcript_notice(
        &mut host_ui_rx,
        "Collaborator connected: peer #1 (co-pilot, 1 peer(s) total)",
    )
    .await;

    fixture.server.stop().await;
    fixture.guest_ep.close().await;
}
