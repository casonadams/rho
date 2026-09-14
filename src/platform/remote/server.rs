use anyhow::Result;
use iroh::Endpoint;
use rho_engine::auth::AuthStore;
use rho_harness_core::config::Config;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct RemoteServer {
    endpoint: Endpoint,
    config: Arc<RwLock<Config>>,
    auth_store: Arc<RwLock<AuthStore>>,
    engine: Arc<RwLock<rho_engine::engine::AgentEngine>>,
}

impl RemoteServer {
    pub fn new(
        endpoint: Endpoint,
        config: Config,
        auth_store: AuthStore,
        engine: rho_engine::engine::AgentEngine,
    ) -> Self {
        Self {
            endpoint,
            config: Arc::new(RwLock::new(config)),
            auth_store: Arc::new(RwLock::new(auth_store)),
            engine: Arc::new(RwLock::new(engine)),
        }
    }

    pub async fn run_accept_loop(&self) -> Result<()> {
        while let Some(incoming) = self.endpoint.accept().await {
            let conn = match incoming.await {
                Ok(c) => c,
                Err(_) => continue,
            };

            let engine_lock = Arc::clone(&self.engine);
            let config_lock = Arc::clone(&self.config);
            let auth_store_lock = Arc::clone(&self.auth_store);

            tokio::spawn(async move {
                while let Ok((send, recv)) = conn.accept_bi().await {
                    let eng = Arc::clone(&engine_lock);
                    let cfg = Arc::clone(&config_lock);
                    let auth = Arc::clone(&auth_store_lock);

                    tokio::spawn(async move {
                        let _ = crate::cli::rpc::run_rpc_session_over_stream(recv, send, eng, cfg, auth).await;
                    });
                }
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::remote::endpoint::{RHO_ALPN, RhoEndpoint};
    use iroh::SecretKey;
    use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent, RpcRequest, RpcResponse};
    use rho_harness_core::rpc::transport::{JsonLinesReader, JsonLinesWriter};
    use tokio::io::BufReader;

    #[tokio::test]
    async fn test_remote_server_connect_and_node_info() {
        let temp_dir = std::env::temp_dir().join(format!("rho_srv_test_{}", uuid::Uuid::new_v4()));
        let config = Config {
            sessions_dir: temp_dir.join("sessions"),
            auth_file: temp_dir.join("auth.json"),
            model: "mock-model".to_string(),
            provider: "ollama".to_string(),
            ..Config::default()
        };
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let engine = crate::platform::agent_engine_in_dir(config.clone(), auth_store.clone(), temp_dir.clone(), None)
            .await
            .unwrap();

        let server_secret = SecretKey::generate();
        let server_ep = RhoEndpoint::bind(server_secret, None).await.unwrap();
        let ticket = server_ep.ticket().unwrap();

        let server = Arc::new(RemoteServer::new(
            server_ep.endpoint().clone(),
            config,
            auth_store,
            engine,
        ));
        let server_task = tokio::spawn(async move {
            let _ = server.run_accept_loop().await;
        });

        // Client side
        let client_secret = SecretKey::generate();
        let client_ep = RhoEndpoint::bind(client_secret, None).await.unwrap();
        let addr = RhoEndpoint::parse_ticket(&ticket).unwrap();

        let conn = client_ep
            .endpoint()
            .connect(addr, RHO_ALPN)
            .await
            .expect("client failed to connect to server");

        let (send, recv) = conn.open_bi().await.expect("failed to open bi stream");
        let mut writer = JsonLinesWriter::new(send);
        let mut reader = JsonLinesReader::new(BufReader::new(recv));

        // In QUIC, send the request first so the peer's accept_bi unblocks on incoming bytes
        let req = RpcRequest {
            id: Some("req-test-1".to_string()),
            command: RpcCommand::GetNodeInfo,
        };
        writer.write_message(&req).await.unwrap();

        // Read initial SessionStart event
        let first_ev = reader
            .read_message::<RpcEvent>()
            .await
            .unwrap()
            .expect("expected SessionStart");
        assert!(matches!(first_ev, RpcEvent::SessionStart { .. }));

        // Read response to GetNodeInfo
        let resp = reader
            .read_message::<RpcResponse>()
            .await
            .unwrap()
            .expect("expected RpcResponse");
        assert!(resp.success);
        assert_eq!(resp.command, "get_node_info");
        let data = resp.data.expect("expected response data");
        assert!(data.get("hostname").is_some());
        assert!(data.get("os").is_some());

        server_task.abort();
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
