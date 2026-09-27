use std::net::SocketAddr;
use std::sync::Arc;

use iroh::Endpoint;
use iroh::endpoint::presets::Minimal;
use rho_harness_core::collab::{
    COLLAB_ALPN, CapabilityLevel, CollabHostConfig, CollabHostServer, CollabSessionStream, CollabSnapshot,
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
