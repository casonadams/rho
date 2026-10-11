use iroh::Endpoint;
use iroh::endpoint::presets::{Minimal, N0};
use rho_harness_core::agent_mesh::{
    AGENT_TASK_ALPN, AgentClientMessage, AgentTaskEvent, AgentTaskRequest, AgentTicket, AgentWorkerMessage,
    perform_client_handshake,
};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::rpc::transport::{JsonLinesReader, JsonLinesWriter};
use std::net::SocketAddr;
use std::sync::Arc;

pub struct AgentClientEndpoint {
    endpoint: Arc<Endpoint>,
}

impl AgentClientEndpoint {
    #[must_use]
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint: Arc::new(endpoint),
        }
    }

    pub async fn bind(bind_addr: Option<SocketAddr>) -> Result<Self> {
        let builder = match bind_addr {
            Some(addr) => Endpoint::builder(Minimal)
                .bind_addr(addr)
                .map_err(|e| AppError::Network(e.to_string()))?,
            None => Endpoint::builder(N0),
        };
        let ep = builder.bind().await.map_err(|e| AppError::Network(e.to_string()))?;
        Ok(Self::new(ep))
    }

    pub async fn close(&self) {
        self.endpoint.close().await;
    }

    pub async fn run_task<F>(&self, ticket: &AgentTicket, request: AgentTaskRequest, mut on_event: F) -> Result<String>
    where
        F: FnMut(AgentTaskEvent) + Send,
    {
        let conn = self
            .endpoint
            .connect(ticket.to_endpoint_addr(), AGENT_TASK_ALPN)
            .await
            .map_err(|e| AppError::Network(format!("Failed to connect to agent peer: {e}")))?;

        let (send_stream, recv_stream) = conn
            .accept_bi()
            .await
            .map_err(|e| AppError::Network(format!("Failed to accept bi-directional stream: {e}")))?;

        let mut writer = JsonLinesWriter::new(send_stream);
        let mut reader = JsonLinesReader::new(tokio::io::BufReader::new(recv_stream));

        perform_client_handshake(&mut reader, &mut writer, ticket, None).await?;

        let target_task_id = request.task_id.clone();
        writer.write_message(&AgentClientMessage::Task(request)).await?;

        while let Some(worker_msg) = reader.read_message::<AgentWorkerMessage>().await? {
            match worker_msg {
                AgentWorkerMessage::Event { task_id, event } if task_id == target_task_id => match event {
                    AgentTaskEvent::Complete { ref result, turns } => {
                        let text = result.clone();
                        on_event(AgentTaskEvent::Complete {
                            result: text.clone(),
                            turns,
                        });
                        return Ok(text);
                    }
                    AgentTaskEvent::Error { message } => {
                        let err_text = message.clone();
                        on_event(AgentTaskEvent::Error { message });
                        return Err(AppError::Tool(err_text));
                    }
                    other => {
                        on_event(other);
                    }
                },
                _ => {}
            }
        }

        Err(AppError::Session("Agent peer closed stream unexpectedly".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_mesh::server::{AgentWorkerConfig, AgentWorkerServer};
    use rho_harness_core::agent_mesh::AgentCapability;

    #[tokio::test]
    async fn test_client_connect_and_error_handling() {
        let server_config = AgentWorkerConfig {
            capability: AgentCapability::ReadOnly,
            relay_url: None,
            bind_addr: Some("127.0.0.1:0".parse().unwrap()),
            model: None,
            tools: Vec::new(),
        };

        let server = AgentWorkerServer::start(server_config).await.expect("server starts");
        let ticket = server.ticket().clone();

        let client = AgentClientEndpoint::bind(Some("127.0.0.1:0".parse().unwrap()))
            .await
            .expect("client binds");

        let req = AgentTaskRequest {
            task_id: "test-task-1".into(),
            prompt: "Summarize this file".into(),
            context_slice: None,
            max_turns: Some(3),
            role: Some("scout".into()),
        };

        let mut events = Vec::new();
        let res = client.run_task(&ticket, req, |ev| events.push(ev)).await;

        assert!(res.is_err());
        assert!(!events.is_empty());
        assert!(matches!(events.last(), Some(AgentTaskEvent::Error { .. })));

        client.close().await;
        server.close().await;
    }
}
