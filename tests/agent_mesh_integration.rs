use rho_engine::agent_mesh::{AgentClientEndpoint, AgentWorkerConfig, AgentWorkerServer};
use rho_harness_core::agent_mesh::{AgentCapability, AgentTaskEvent, AgentTaskRequest, AgentTicket};
use rho_harness_core::config::Config;
use std::net::SocketAddr;

#[tokio::test]
async fn test_agent_mesh_end_to_end_readonly_enforcement() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config = Config::default();
    let tools = rho_engine::tools::build_builtin_tools(temp_dir.path(), &config).expect("tools build");

    let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server_config = AgentWorkerConfig {
        capability: AgentCapability::ReadOnly,
        relay_url: None,
        bind_addr: Some(bind_addr),
        model: None,
        tools,
    };

    let server = AgentWorkerServer::start(server_config)
        .await
        .expect("worker starts successfully");

    let ticket_uri = server.ticket().to_uri();
    assert!(ticket_uri.starts_with("rho-agent://"));
    assert!(ticket_uri.contains("cap=read_only"));

    let parsed_ticket = AgentTicket::parse(&ticket_uri).expect("parses ticket");
    assert_eq!(parsed_ticket.capability, AgentCapability::ReadOnly);

    let client = AgentClientEndpoint::bind(Some("127.0.0.1:0".parse().unwrap()))
        .await
        .expect("client binds");

    let request = AgentTaskRequest {
        task_id: "test-task-mesh-1".into(),
        prompt: "Inspect code".into(),
        context_slice: None,
        max_turns: Some(2),
        role: Some("scout".into()),
    };

    let mut received_events = Vec::new();
    let result = client
        .run_task(&parsed_ticket, request, |ev| received_events.push(ev))
        .await;

    assert!(result.is_err());
    assert!(
        received_events
            .iter()
            .any(|e| matches!(e, AgentTaskEvent::Error { .. }))
    );

    client.close().await;
    server.close().await;
}
