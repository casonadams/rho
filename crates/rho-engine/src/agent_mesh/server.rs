use crate::engine::compactor::llm::ModelHandle;
use crate::tools::subagent::SubagentRunner;
use crate::tools::subagent::types::SubagentRole;
use iroh::endpoint::presets::{Minimal, N0};
use iroh::{Endpoint, EndpointId, RelayUrl};
use rho_harness_core::agent_mesh::{
    AGENT_TASK_ALPN, AgentCapability, AgentClientMessage, AgentSecret, AgentTaskEvent, AgentTaskRequest, AgentTicket,
    AgentWorkerMessage, perform_worker_handshake,
};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::rpc::transport::{JsonLinesReader, JsonLinesWriter};
use rig::tool::DynamicTool;
use std::net::SocketAddr;

pub struct AgentWorkerConfig {
    pub capability: AgentCapability,
    pub relay_url: Option<RelayUrl>,
    pub bind_addr: Option<SocketAddr>,
    pub model: Option<ModelHandle>,
    pub tools: Vec<DynamicTool>,
}

pub struct AgentWorkerServer {
    endpoint: Endpoint,
    ticket: AgentTicket,
    accept_task: tokio::task::JoinHandle<()>,
}

impl AgentWorkerServer {
    pub async fn start(config: AgentWorkerConfig) -> Result<Self> {
        let secret = AgentSecret::generate().map_err(|_| AppError::Auth("failed to generate agent secret".into()))?;
        let derived_key = secret
            .derive_key(config.capability)
            .map_err(|_| AppError::Auth("failed to derive capability key".into()))?;

        let endpoint = if let Some(addr) = config.bind_addr {
            Endpoint::builder(Minimal)
                .alpns(vec![AGENT_TASK_ALPN.to_vec()])
                .bind_addr(addr)
                .map_err(|e| AppError::Network(e.to_string()))?
                .bind()
                .await
                .map_err(|e| AppError::Network(e.to_string()))?
        } else {
            Endpoint::builder(N0)
                .alpns(vec![AGENT_TASK_ALPN.to_vec()])
                .bind()
                .await
                .map_err(|e| AppError::Network(e.to_string()))?
        };

        let mut ticket = AgentTicket::new(endpoint.id(), config.relay_url, config.capability, derived_key);
        if let Some(direct) = endpoint.addr().ip_addrs().next() {
            ticket = ticket.with_direct_addr(*direct);
        }

        let accept_task = tokio::spawn(run_worker_accept_loop(
            endpoint.clone(),
            secret,
            config.model,
            config.tools,
        ));

        Ok(Self {
            endpoint,
            ticket,
            accept_task,
        })
    }

    #[must_use]
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    #[must_use]
    pub fn ticket(&self) -> &AgentTicket {
        &self.ticket
    }

    #[must_use]
    pub fn endpoint_id(&self) -> EndpointId {
        self.endpoint.id()
    }

    pub async fn close(&self) {
        self.endpoint.close().await;
    }
}

impl Drop for AgentWorkerServer {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

async fn run_worker_accept_loop(
    endpoint: Endpoint,
    secret: AgentSecret,
    model: Option<ModelHandle>,
    tools: Vec<DynamicTool>,
) {
    while let Some(incoming) = endpoint.accept().await {
        let s_clone = secret.clone();
        let m_clone = model.clone();
        let t_clone = tools.clone();
        tokio::spawn(async move {
            if let Ok(connecting) = incoming.await {
                handle_worker_connection(connecting, s_clone, m_clone, t_clone).await;
            }
        });
    }
}

async fn handle_worker_connection(
    conn: iroh::endpoint::Connection,
    secret: AgentSecret,
    model: Option<ModelHandle>,
    tools: Vec<DynamicTool>,
) {
    let (send_stream, recv_stream) = match conn.open_bi().await {
        Ok(streams) => streams,
        Err(_) => return,
    };
    let mut reader = JsonLinesReader::new(tokio::io::BufReader::new(recv_stream));
    let mut writer = JsonLinesWriter::new(send_stream);

    let (granted_cap, _client_id) = match perform_worker_handshake(&mut reader, &mut writer, &secret).await {
        Ok(res) => res,
        Err(_) => return,
    };

    while let Ok(Some(client_msg)) = reader.read_message::<AgentClientMessage>().await {
        match client_msg {
            AgentClientMessage::Task(req) => {
                let event = execute_worker_task(&req, granted_cap, model.as_ref(), &tools).await;
                let worker_msg = AgentWorkerMessage::Event {
                    task_id: req.task_id,
                    event,
                };
                let _ = writer.write_message(&worker_msg).await;
            }
            AgentClientMessage::Cancel { .. } => break,
        }
    }
}

async fn execute_worker_task(
    req: &AgentTaskRequest,
    granted_cap: AgentCapability,
    model: Option<&ModelHandle>,
    tools: &[DynamicTool],
) -> AgentTaskEvent {
    let model = match model {
        Some(m) => m.clone(),
        None => {
            return AgentTaskEvent::Error {
                message: "No LLM model configured on this agent worker".into(),
            };
        }
    };

    let role = req
        .role
        .as_deref()
        .and_then(|r| r.parse::<SubagentRole>().ok())
        .unwrap_or(SubagentRole::General);

    let safe_tools: Vec<DynamicTool> = tools
        .iter()
        .filter(|t| granted_cap.is_tool_allowed(t.name()))
        .cloned()
        .collect();

    let max_turns = req.max_turns.unwrap_or(8).min(15);
    let runner = SubagentRunner::new(model, role, &safe_tools, max_turns);
    let result = runner.run(&req.prompt, req.context_slice.as_deref()).await;

    if result.is_error {
        AgentTaskEvent::Error {
            message: result.content,
        }
    } else {
        AgentTaskEvent::Complete {
            result: result.content,
            turns: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_worker_server_start_and_ticket_generation() {
        let config = AgentWorkerConfig {
            capability: AgentCapability::ReadOnly,
            relay_url: None,
            bind_addr: Some("127.0.0.1:0".parse().unwrap()),
            model: None,
            tools: Vec::new(),
        };

        let server = AgentWorkerServer::start(config).await.expect("worker starts");
        let ticket = server.ticket();
        assert_eq!(ticket.capability, AgentCapability::ReadOnly);
        assert_eq!(ticket.endpoint_id, server.endpoint_id());
        assert!(ticket.to_uri().starts_with("rho-agent://"));
        server.close().await;
    }
}
