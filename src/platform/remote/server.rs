use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use iroh::Endpoint;
use rho_engine::auth::AuthStore;
use rho_harness_core::config::Config;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_websockets::{Message, ServerBuilder};

pub async fn bind_ws_listener(port: Option<u16>) -> Result<(TcpListener, u16)> {
    if let Some(p) = port {
        let listener = TcpListener::bind(("0.0.0.0", p)).await?;
        let actual_port = listener.local_addr()?.port();
        return Ok((listener, actual_port));
    }

    for p in [50051, 50052, 50053, 0] {
        if let Ok(listener) = TcpListener::bind(("0.0.0.0", p)).await {
            let actual_port = listener.local_addr()?.port();
            return Ok((listener, actual_port));
        }
    }

    let listener = TcpListener::bind(("0.0.0.0", 0)).await?;
    let actual_port = listener.local_addr()?.port();
    Ok((listener, actual_port))
}

pub struct RemoteServer {
    endpoint: Endpoint,
    ws_listener: Option<TcpListener>,
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
            ws_listener: None,
            config: Arc::new(RwLock::new(config)),
            auth_store: Arc::new(RwLock::new(auth_store)),
            engine: Arc::new(RwLock::new(engine)),
        }
    }

    pub fn with_ws_listener(mut self, listener: TcpListener) -> Self {
        self.ws_listener = Some(listener);
        self
    }

    async fn run_iroh_loop(&self) -> Result<()> {
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

    async fn run_ws_loop(&self, listener: &TcpListener) -> Result<()> {
        while let Ok((stream, _)) = listener.accept().await {
            let eng = Arc::clone(&self.engine);
            let cfg = Arc::clone(&self.config);
            let auth = Arc::clone(&self.auth_store);

            tokio::spawn(async move {
                Self::handle_ws_client(stream, eng, cfg, auth).await;
            });
        }
        Ok(())
    }

    async fn handle_ws_client(
        stream: tokio::net::TcpStream,
        engine: Arc<RwLock<rho_engine::engine::AgentEngine>>,
        config: Arc<RwLock<Config>>,
        auth_store: Arc<RwLock<AuthStore>>,
    ) {
        let Ok((_request, mut ws_stream)) = ServerBuilder::new().accept(stream).await else {
            return;
        };

        let (client_io, server_io) = tokio::io::duplex(65536);
        let (mut client_read, mut client_write) = tokio::io::split(client_io);
        let (server_read, server_write) = tokio::io::split(server_io);

        let rpc_task = tokio::spawn(async move {
            let _ = crate::cli::rpc::run_rpc_session_over_stream(server_read, server_write, engine, config, auth_store)
                .await;
        });

        let mut lines = BufReader::new(&mut client_read).lines();

        loop {
            tokio::select! {
                line_res = lines.next_line() => {
                    match line_res {
                        Ok(Some(line)) => {
                            if ws_stream.send(Message::text(line)).await.is_err() {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
                msg_res = ws_stream.next() => {
                    match msg_res {
                        Some(Ok(msg)) => {
                            if let Some(text) = msg.as_text()
                                && (client_write.write_all(text.as_bytes()).await.is_err()
                                    || client_write.write_all(b"\n").await.is_err()
                                    || client_write.flush().await.is_err())
                            {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
            }
        }

        rpc_task.abort();
    }

    pub async fn run_accept_loop(&self) -> Result<()> {
        let iroh_task = self.run_iroh_loop();
        if let Some(ref listener) = self.ws_listener {
            let ws_task = self.run_ws_loop(listener);
            tokio::select! {
                r = iroh_task => r,
                r = ws_task => r,
            }
        } else {
            iroh_task.await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::remote::endpoint::{RHO_ALPN, RhoEndpoint};
    use iroh::SecretKey;
    use rho_harness_core::rpc::protocol::{RpcCommand, RpcEvent, RpcRequest, RpcResponse};
    use rho_harness_core::rpc::transport::{JsonLinesReader, JsonLinesWriter};

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

        let req = RpcRequest {
            id: Some("req-test-1".to_string()),
            command: RpcCommand::GetNodeInfo,
        };
        writer.write_message(&req).await.unwrap();

        let first_ev = reader
            .read_message::<RpcEvent>()
            .await
            .unwrap()
            .expect("expected SessionStart");
        assert!(matches!(first_ev, RpcEvent::SessionStart { .. }));

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

    #[tokio::test]
    async fn test_remote_server_websocket_flow() {
        let temp_dir = std::env::temp_dir().join(format!("rho_ws_test_{}", uuid::Uuid::new_v4()));
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

        let (ws_listener, ws_port) = bind_ws_listener(None).await.unwrap();

        let server = Arc::new(
            RemoteServer::new(server_ep.endpoint().clone(), config, auth_store, engine).with_ws_listener(ws_listener),
        );
        let server_task = tokio::spawn(async move {
            let _ = server.run_accept_loop().await;
        });

        let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{ws_port}"))
            .await
            .unwrap();
        let uri = http::Uri::try_from(format!("ws://127.0.0.1:{ws_port}")).unwrap();
        let (mut client_ws, _) = tokio_websockets::ClientBuilder::from_uri(uri)
            .connect_on(stream)
            .await
            .unwrap();

        let req = RpcRequest {
            id: Some("ws-test-1".to_string()),
            command: RpcCommand::GetNodeInfo,
        };
        let json = serde_json::to_string(&req).unwrap();
        client_ws.send(Message::text(json)).await.unwrap();

        let mut got_node_info = false;
        while let Some(Ok(msg)) = client_ws.next().await {
            if let Some(text) = msg.as_text() {
                if text.contains("session_start") {
                    continue;
                }
                let resp: RpcResponse = serde_json::from_str(text).unwrap();
                if resp.command == "get_node_info" {
                    assert!(resp.success);
                    got_node_info = true;
                    break;
                }
            }
        }
        assert!(got_node_info);

        server_task.abort();
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
