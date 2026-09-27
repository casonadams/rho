use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use iroh::endpoint::presets::{Minimal, N0};
use iroh::{Endpoint, EndpointId, RelayUrl};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{Mutex, RwLock, broadcast, mpsc, oneshot};

use crate::collab::crypto::{CapabilityLevel, CollabSecret};
use crate::collab::protocol::{COLLAB_ALPN, CollabSnapshot};
use crate::collab::session::{CollabReader, CollabSessionStream, CollabWriter};
use crate::collab::ticket::CollabTicket;
use crate::error::{AppError, Result};
use crate::rpc::protocol::{RpcCommand, RpcEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollabPeerInfo {
    pub id: usize,
    pub role: CapabilityLevel,
    pub hostname: Option<String>,
    pub connected_at: chrono::DateTime<chrono::Utc>,
}

impl CollabPeerInfo {
    #[must_use]
    pub fn new(
        id: usize,
        role: CapabilityLevel,
        hostname: Option<String>,
        connected_at: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Self {
            id,
            role,
            hostname,
            connected_at,
        }
    }

    #[must_use]
    pub fn display_name(&self) -> String {
        match &self.hostname {
            Some(h) if !h.is_empty() => h.clone(),
            _ => format!("peer #{}", self.id),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CollabIncomingCommand {
    pub peer_id: usize,
    pub role: CapabilityLevel,
    pub command: RpcCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollabPeerEvent {
    Connected(CollabPeerInfo),
    Disconnected {
        peer_id: usize,
        display_name: String,
        reason: String,
    },
}

struct ActivePeer {
    info: CollabPeerInfo,
    sender: mpsc::Sender<RpcEvent>,
    disconnect_tx: Option<oneshot::Sender<()>>,
}

struct CollabServerState {
    secret: CollabSecret,
    peers: HashMap<usize, ActivePeer>,
    next_peer_id: usize,
    relay_url: Option<RelayUrl>,
    snapshot_provider: Arc<dyn Fn() -> CollabSnapshot + Send + Sync>,
}

#[derive(Clone, Default)]
pub struct CollabHostConfig {
    pub secret: Option<CollabSecret>,
    pub bind_addr: Option<SocketAddr>,
    pub relay_url: Option<RelayUrl>,
    pub snapshot_provider: Option<Arc<dyn Fn() -> CollabSnapshot + Send + Sync>>,
}

impl CollabHostConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_secret(mut self, secret: CollabSecret) -> Self {
        self.secret = Some(secret);
        self
    }

    #[must_use]
    pub fn with_bind_addr(mut self, addr: SocketAddr) -> Self {
        self.bind_addr = Some(addr);
        self
    }

    #[must_use]
    pub fn with_relay_url(mut self, url: RelayUrl) -> Self {
        self.relay_url = Some(url);
        self
    }

    #[must_use]
    pub fn with_snapshot_provider(mut self, provider: impl Fn() -> CollabSnapshot + Send + Sync + 'static) -> Self {
        self.snapshot_provider = Some(Arc::new(provider));
        self
    }
}

pub struct CollabHostServer {
    endpoint: Endpoint,
    state: Arc<RwLock<CollabServerState>>,
    command_rx: Mutex<mpsc::Receiver<CollabIncomingCommand>>,
    peer_event_rx: Mutex<mpsc::Receiver<CollabPeerEvent>>,
    shutdown_tx: broadcast::Sender<()>,
    accept_task: tokio::task::JoinHandle<()>,
}

fn test_loopback_addr() -> Option<SocketAddr> {
    if cfg!(test)
        || std::env::var_os("RUST_TEST_THREADS").is_some()
        || std::thread::current().name().is_some_and(|n| n.contains("::"))
    {
        Some(SocketAddr::from(([127, 0, 0, 1], 0)))
    } else {
        None
    }
}

async fn bind_collab_endpoint(bind_addr: Option<SocketAddr>) -> Result<Endpoint> {
    let effective_addr = bind_addr.or_else(test_loopback_addr);
    if let Some(addr) = effective_addr {
        Endpoint::builder(Minimal)
            .bind_addr(addr)
            .map_err(|e| AppError::Network(e.to_string()))?
            .alpns(vec![COLLAB_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| AppError::Network(e.to_string()))
    } else {
        Endpoint::builder(N0)
            .alpns(vec![COLLAB_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| AppError::Network(e.to_string()))
    }
}

impl CollabHostServer {
    pub async fn start(config: CollabHostConfig) -> Result<Self> {
        let secret = match config.secret {
            Some(s) => s,
            None => CollabSecret::generate().map_err(|_| AppError::Auth("Failed to generate secret".into()))?,
        };

        let endpoint = bind_collab_endpoint(config.bind_addr).await?;

        let snapshot_provider = config
            .snapshot_provider
            .unwrap_or_else(|| Arc::new(|| CollabSnapshot::new(Vec::new(), "idle")));

        let state = Arc::new(RwLock::new(CollabServerState {
            secret,
            peers: HashMap::new(),
            next_peer_id: 1,
            relay_url: config.relay_url,
            snapshot_provider,
        }));

        let (command_tx, command_rx) = mpsc::channel(64);
        let (peer_event_tx, peer_event_rx) = mpsc::channel(64);
        let (shutdown_tx, _) = broadcast::channel(1);

        let accept_task = tokio::spawn(run_accept_loop(
            endpoint.clone(),
            state.clone(),
            command_tx,
            peer_event_tx,
            shutdown_tx.subscribe(),
        ));

        Ok(Self {
            endpoint,
            state,
            command_rx: Mutex::new(command_rx),
            peer_event_rx: Mutex::new(peer_event_rx),
            shutdown_tx,
            accept_task,
        })
    }

    #[must_use]
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    #[must_use]
    pub fn endpoint_id(&self) -> EndpointId {
        self.endpoint.id()
    }

    pub async fn tickets(&self) -> Result<(CollabTicket, CollabTicket)> {
        let guard = self.state.read().await;
        let endpoint_id = self.endpoint.id();
        let full = CollabTicket::new(endpoint_id, guard.relay_url.clone(), false, *guard.secret.seed());
        let read_key = guard
            .secret
            .derive_read_key()
            .map_err(|_| AppError::Auth("Failed to derive read key".into()))?;
        let view = CollabTicket::new(endpoint_id, guard.relay_url.clone(), true, read_key);
        Ok((full, view))
    }

    pub async fn peers(&self) -> Vec<CollabPeerInfo> {
        let guard = self.state.read().await;
        let mut list: Vec<CollabPeerInfo> = guard.peers.values().map(|p| p.info.clone()).collect();
        list.sort_by_key(|p| p.id);
        list
    }

    pub async fn peer_count(&self) -> usize {
        self.state.read().await.peers.len()
    }

    pub fn peers_sync(&self) -> Vec<CollabPeerInfo> {
        let Ok(guard) = self.state.try_read() else {
            return Vec::new();
        };
        let mut list: Vec<CollabPeerInfo> = guard.peers.values().map(|p| p.info.clone()).collect();
        list.sort_by_key(|p| p.id);
        list
    }

    pub fn peer_count_sync(&self) -> usize {
        self.state.try_read().map(|g| g.peers.len()).unwrap_or(0)
    }

    pub fn broadcast(&self, event: &RpcEvent) {
        if let Ok(guard) = self.state.try_read() {
            for peer in guard.peers.values() {
                let _ = peer.sender.try_send(event.clone());
            }
        }
    }

    pub async fn broadcast_async(&self, event: &RpcEvent) {
        let guard = self.state.read().await;
        for peer in guard.peers.values() {
            let _ = peer.sender.send(event.clone()).await;
        }
    }

    pub async fn send_to_peer(&self, peer_id: usize, event: &RpcEvent) -> bool {
        let guard = self.state.read().await;
        if let Some(peer) = guard.peers.get(&peer_id) {
            peer.sender.try_send(event.clone()).is_ok()
        } else {
            false
        }
    }

    pub async fn kick_peer(&self, peer_id: usize) -> bool {
        let mut guard = self.state.write().await;
        if let Some(mut peer) = guard.peers.remove(&peer_id) {
            let err_event = RpcEvent::Error {
                code: "KICKED".into(),
                message: "Kicked by host".into(),
            };
            let _ = peer.sender.try_send(err_event);
            if let Some(tx) = peer.disconnect_tx.take() {
                let _ = tx.send(());
            }
            true
        } else {
            false
        }
    }

    pub async fn kick_peer_by_spec(&self, spec: &str) -> bool {
        let trimmed = spec.trim().trim_start_matches('#');
        if let Ok(id) = trimmed.parse::<usize>() {
            self.kick_peer(id).await
        } else {
            false
        }
    }

    pub async fn rotate_secret(&self) -> Result<(CollabTicket, CollabTicket)> {
        let new_secret = CollabSecret::generate().map_err(|_| AppError::Auth("Failed to generate secret".into()))?;
        let (full, view) = {
            let mut guard = self.state.write().await;
            for (_, mut peer) in guard.peers.drain() {
                let err_event = RpcEvent::Error {
                    code: "ROTATED".into(),
                    message: "Host rotated session keys".into(),
                };
                let _ = peer.sender.try_send(err_event);
                if let Some(tx) = peer.disconnect_tx.take() {
                    let _ = tx.send(());
                }
            }
            guard.secret = new_secret;
            let endpoint_id = self.endpoint.id();
            let full = CollabTicket::new(endpoint_id, guard.relay_url.clone(), false, *guard.secret.seed());
            let read_key = guard
                .secret
                .derive_read_key()
                .map_err(|_| AppError::Auth("Failed to derive read key".into()))?;
            let view = CollabTicket::new(endpoint_id, guard.relay_url.clone(), true, read_key);
            (full, view)
        };
        Ok((full, view))
    }

    pub async fn set_snapshot_provider(&self, provider: impl Fn() -> CollabSnapshot + Send + Sync + 'static) {
        let mut guard = self.state.write().await;
        guard.snapshot_provider = Arc::new(provider);
    }

    pub async fn update_snapshot(&self, snapshot: CollabSnapshot) {
        let snap_arc = Arc::new(snapshot);
        self.set_snapshot_provider(move || (*snap_arc).clone()).await;
    }

    pub async fn recv_command(&self) -> Option<CollabIncomingCommand> {
        self.command_rx.lock().await.recv().await
    }

    pub async fn recv_peer_event(&self) -> Option<CollabPeerEvent> {
        self.peer_event_rx.lock().await.recv().await
    }

    pub async fn stop(&self) {
        let _ = self.shutdown_tx.send(());
        self.accept_task.abort();
        {
            let mut guard = self.state.write().await;
            for (_, mut peer) in guard.peers.drain() {
                let err_event = RpcEvent::Error {
                    code: "STOPPED".into(),
                    message: "Collab session stopped by host".into(),
                };
                let _ = peer.sender.try_send(err_event);
                if let Some(tx) = peer.disconnect_tx.take() {
                    let _ = tx.send(());
                }
            }
        }
        self.endpoint.close().await;
    }
}

impl Drop for CollabHostServer {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        self.accept_task.abort();
    }
}

async fn run_accept_loop(
    endpoint: Endpoint,
    state: Arc<RwLock<CollabServerState>>,
    command_tx: mpsc::Sender<CollabIncomingCommand>,
    peer_event_tx: mpsc::Sender<CollabPeerEvent>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                break;
            }
            incoming_opt = endpoint.accept() => {
                let Some(incoming) = incoming_opt else {
                    break;
                };
                let state_clone = state.clone();
                let command_tx_clone = command_tx.clone();
                let peer_event_tx_clone = peer_event_tx.clone();
                tokio::spawn(async move {
                    let _ = handle_incoming_connection(
                        incoming,
                        state_clone,
                        command_tx_clone,
                        peer_event_tx_clone,
                    ).await;
                });
            }
        }
    }
}

async fn handle_incoming_connection(
    incoming: iroh::endpoint::Incoming,
    state: Arc<RwLock<CollabServerState>>,
    command_tx: mpsc::Sender<CollabIncomingCommand>,
    peer_event_tx: mpsc::Sender<CollabPeerEvent>,
) -> Result<()> {
    let conn = incoming
        .accept()
        .map_err(|e| AppError::Network(e.to_string()))?
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    let (send, recv) = conn.open_bi().await.map_err(|e| AppError::Network(e.to_string()))?;

    let secret = {
        let guard = state.read().await;
        guard.secret.clone()
    };

    let mut session_stream = CollabSessionStream::accept_host(send, recv, &secret).await?;
    let role = session_stream.role();
    let peer_hostname = session_stream.peer_hostname().map(ToString::to_string);

    let snapshot = {
        let guard = state.read().await;
        (guard.snapshot_provider)()
    };
    session_stream.send_snapshot(&snapshot).await?;

    let (writer, reader) = session_stream.into_split();

    let (peer_id, info, event_rx, disconnect_rx) = {
        let mut guard = state.write().await;
        let id = guard.next_peer_id;
        guard.next_peer_id += 1;
        let info = CollabPeerInfo {
            id,
            role,
            hostname: peer_hostname,
            connected_at: chrono::Utc::now(),
        };
        let (event_tx, event_rx) = mpsc::channel(128);
        let (disconnect_tx, disconnect_rx) = oneshot::channel();
        guard.peers.insert(
            id,
            ActivePeer {
                info: info.clone(),
                sender: event_tx,
                disconnect_tx: Some(disconnect_tx),
            },
        );
        (id, info, event_rx, disconnect_rx)
    };

    let _ = peer_event_tx.send(CollabPeerEvent::Connected(info.clone())).await;

    let ctx = PeerSessionContext {
        peer_id,
        display_name: info.display_name(),
        role,
        writer,
        reader,
        event_rx,
        disconnect_rx,
    };

    run_peer_session(ctx, command_tx, peer_event_tx, state).await;

    Ok(())
}

struct PeerSessionContext<W, R> {
    peer_id: usize,
    display_name: String,
    role: CapabilityLevel,
    writer: CollabWriter<W>,
    reader: CollabReader<R>,
    event_rx: mpsc::Receiver<RpcEvent>,
    disconnect_rx: oneshot::Receiver<()>,
}

async fn run_peer_session<W: AsyncWrite + Unpin + Send + 'static, R: AsyncRead + Unpin + Send + 'static>(
    ctx: PeerSessionContext<W, R>,
    command_tx: mpsc::Sender<CollabIncomingCommand>,
    peer_event_tx: mpsc::Sender<CollabPeerEvent>,
    state: Arc<RwLock<CollabServerState>>,
) {
    let (per_peer_tx, per_peer_rx) = mpsc::channel(64);
    let mut write_task = tokio::spawn(run_peer_write_loop(ctx.writer, ctx.event_rx, per_peer_rx));
    let mut read_task = tokio::spawn(run_peer_read_loop(
        ctx.peer_id,
        ctx.role,
        ctx.reader,
        command_tx,
        per_peer_tx,
    ));

    tokio::select! {
        _ = &mut write_task => {},
        _ = &mut read_task => {},
        _ = ctx.disconnect_rx => {},
    }

    write_task.abort();
    read_task.abort();

    {
        let mut guard = state.write().await;
        guard.peers.remove(&ctx.peer_id);
    }

    let _ = peer_event_tx
        .send(CollabPeerEvent::Disconnected {
            peer_id: ctx.peer_id,
            display_name: ctx.display_name,
            reason: "Stream closed".into(),
        })
        .await;
}

async fn run_peer_write_loop<W: AsyncWrite + Unpin>(
    mut writer: CollabWriter<W>,
    mut event_rx: mpsc::Receiver<RpcEvent>,
    mut per_peer_rx: mpsc::Receiver<RpcEvent>,
) {
    loop {
        tokio::select! {
            event_opt = event_rx.recv() => {
                let Some(event) = event_opt else { break; };
                if writer.send_event(&event).await.is_err() {
                    break;
                }
            }
            event_opt = per_peer_rx.recv() => {
                let Some(event) = event_opt else { break; };
                if writer.send_event(&event).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn run_peer_read_loop<R: AsyncRead + Unpin>(
    peer_id: usize,
    role: CapabilityLevel,
    mut reader: CollabReader<R>,
    command_tx: mpsc::Sender<CollabIncomingCommand>,
    per_peer_tx: mpsc::Sender<RpcEvent>,
) {
    while let Ok(Some(cmd)) = reader.recv_raw::<RpcCommand>().await {
        if role == CapabilityLevel::ViewOnly {
            let err_event = RpcEvent::Error {
                code: "UNAUTHORIZED".into(),
                message: "View-only collaborators cannot execute commands".into(),
            };
            let _ = per_peer_tx.send(err_event).await;
        } else {
            let incoming = CollabIncomingCommand {
                peer_id,
                role,
                command: cmd,
            };
            if command_tx.send(incoming).await.is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_start_stop() {
        let secret = CollabSecret::generate().expect("secret");
        let config = CollabHostConfig::new()
            .with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .with_secret(secret);

        let server = CollabHostServer::start(config).await.expect("start server");
        assert_eq!(server.peer_count().await, 0);

        let (full_ticket, view_ticket) = server.tickets().await.expect("tickets");
        assert!(!full_ticket.is_view_only);
        assert!(view_ticket.is_view_only);
        assert_eq!(full_ticket.endpoint_id, server.endpoint_id());
        assert_eq!(view_ticket.endpoint_id, server.endpoint_id());

        server.stop().await;
    }

    #[tokio::test]
    async fn test_server_full_peer_command_routing() {
        let secret = CollabSecret::generate().expect("secret");
        let config = CollabHostConfig::new()
            .with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .with_secret(secret)
            .with_snapshot_provider(|| {
                CollabSnapshot::new(vec![serde_json::json!({"turn": 1, "prompt": "initial"})], "idle")
            });

        let server = CollabHostServer::start(config).await.expect("start server");
        let (full_ticket, _) = server.tickets().await.expect("tickets");

        let guest_ep = Endpoint::builder(Minimal)
            .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("bind guest addr")
            .bind()
            .await
            .expect("guest bind");

        let guest_conn = guest_ep
            .connect(server.endpoint().addr(), COLLAB_ALPN)
            .await
            .expect("guest connect");
        let (send, recv) = guest_conn.accept_bi().await.expect("accept_bi");

        let mut guest_stream = CollabSessionStream::connect_guest_with_ticket(send, recv, &full_ticket)
            .await
            .expect("connect guest");
        assert_eq!(guest_stream.role(), CapabilityLevel::Full);

        let connect_ev = server.recv_peer_event().await;
        assert!(
            matches!(connect_ev, Some(CollabPeerEvent::Connected(info)) if info.id == 1 && info.role == CapabilityLevel::Full)
        );
        assert_eq!(server.peer_count().await, 1);

        let snap = guest_stream.recv_snapshot().await.expect("recv snapshot");
        assert_eq!(
            snap,
            Some(CollabSnapshot::new(
                vec![serde_json::json!({"turn": 1, "prompt": "initial"})],
                "idle"
            ))
        );

        server.broadcast(&RpcEvent::TextChunk {
            content: "server broadcast".into(),
        });
        let ev = guest_stream.recv_event().await.expect("recv event");
        assert!(matches!(ev, Some(RpcEvent::TextChunk { content }) if content == "server broadcast"));

        guest_stream
            .send_command(&RpcCommand::Prompt {
                message: "execute prompt".into(),
                images: None,
                streaming_behavior: None,
            })
            .await
            .expect("send command");

        let incoming_cmd = server.recv_command().await;
        assert!(matches!(
            incoming_cmd,
            Some(CollabIncomingCommand {
                peer_id: 1,
                role: CapabilityLevel::Full,
                command: RpcCommand::Prompt { message, .. }
            }) if message == "execute prompt"
        ));

        server.stop().await;
        guest_ep.close().await;
    }

    #[tokio::test]
    async fn test_server_view_only_rejection() {
        let secret = CollabSecret::generate().expect("secret");
        let config = CollabHostConfig::new()
            .with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .with_secret(secret);

        let server = CollabHostServer::start(config).await.expect("start server");
        let (_, view_ticket) = server.tickets().await.expect("tickets");

        let guest_ep = Endpoint::builder(Minimal)
            .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("bind guest addr")
            .bind()
            .await
            .expect("guest bind");

        let guest_conn = guest_ep
            .connect(server.endpoint().addr(), COLLAB_ALPN)
            .await
            .expect("guest connect");
        let (send, recv) = guest_conn.accept_bi().await.expect("accept_bi");

        let mut guest_stream = CollabSessionStream::connect_guest_with_ticket(send, recv, &view_ticket)
            .await
            .expect("connect guest");
        assert_eq!(guest_stream.role(), CapabilityLevel::ViewOnly);

        let _ = guest_stream.recv_snapshot().await;

        guest_stream
            .writer
            .send_raw(&RpcCommand::Prompt {
                message: "forged prompt".into(),
                images: None,
                streaming_behavior: None,
            })
            .await
            .expect("raw send");

        let ev = guest_stream.recv_event().await.expect("recv error event");
        assert!(matches!(ev, Some(RpcEvent::Error { code, .. }) if code == "UNAUTHORIZED"));

        let peers = server.peers().await;
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].role, CapabilityLevel::ViewOnly);

        server.stop().await;
        guest_ep.close().await;
    }

    #[tokio::test]
    async fn test_server_kick_peer() {
        let secret = CollabSecret::generate().expect("secret");
        let config = CollabHostConfig::new()
            .with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .with_secret(secret);

        let server = CollabHostServer::start(config).await.expect("start server");
        let (full_ticket, _) = server.tickets().await.expect("tickets");

        let guest_ep = Endpoint::builder(Minimal)
            .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("bind guest addr")
            .bind()
            .await
            .expect("guest bind");

        let guest_conn = guest_ep
            .connect(server.endpoint().addr(), COLLAB_ALPN)
            .await
            .expect("guest connect");
        let (send, recv) = guest_conn.accept_bi().await.expect("accept_bi");

        let mut guest_stream = CollabSessionStream::connect_guest_with_ticket(send, recv, &full_ticket)
            .await
            .expect("connect guest");

        let _ = guest_stream.recv_snapshot().await;
        assert_eq!(server.peer_count().await, 1);

        let kicked = server.kick_peer_by_spec("#1").await;
        assert!(kicked);
        assert_eq!(server.peer_count().await, 0);

        let ev = guest_stream.recv_event().await.expect("recv kick event");
        assert!(matches!(ev, Some(RpcEvent::Error { code, .. }) if code == "KICKED"));

        let disconnect_ev = server.recv_peer_event().await;
        let second_ev = server.recv_peer_event().await;
        assert!(
            matches!(disconnect_ev, Some(CollabPeerEvent::Disconnected { peer_id: 1, .. }))
                || matches!(second_ev, Some(CollabPeerEvent::Disconnected { peer_id: 1, .. }))
        );

        server.stop().await;
        guest_ep.close().await;
    }

    #[tokio::test]
    async fn test_server_rotate_secret() {
        let secret = CollabSecret::generate().expect("secret");
        let config = CollabHostConfig::new()
            .with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .with_secret(secret);

        let server = CollabHostServer::start(config).await.expect("start server");
        let (old_ticket, _) = server.tickets().await.expect("tickets");

        let guest_ep = Endpoint::builder(Minimal)
            .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("bind guest addr")
            .bind()
            .await
            .expect("guest bind");

        let guest_conn = guest_ep
            .connect(server.endpoint().addr(), COLLAB_ALPN)
            .await
            .expect("guest connect");
        let (send, recv) = guest_conn.accept_bi().await.expect("accept_bi");

        let mut guest_stream = CollabSessionStream::connect_guest_with_ticket(send, recv, &old_ticket)
            .await
            .expect("connect guest");

        let _ = guest_stream.recv_snapshot().await;
        assert_eq!(server.peer_count().await, 1);

        let (new_full_ticket, _) = server.rotate_secret().await.expect("rotate");
        assert_ne!(old_ticket.secret, new_full_ticket.secret);
        assert_eq!(server.peer_count().await, 0);

        let ev = guest_stream.recv_event().await.expect("recv rotate event");
        assert!(matches!(ev, Some(RpcEvent::Error { code, .. }) if code == "ROTATED"));

        let guest_conn2 = guest_ep
            .connect(server.endpoint().addr(), COLLAB_ALPN)
            .await
            .expect("guest connect");
        let (send2, recv2) = guest_conn2.accept_bi().await.expect("accept_bi");
        let old_handshake_res = CollabSessionStream::connect_guest_with_ticket(send2, recv2, &old_ticket).await;
        assert!(old_handshake_res.is_err());

        let guest_conn3 = guest_ep
            .connect(server.endpoint().addr(), COLLAB_ALPN)
            .await
            .expect("guest connect");
        let (send3, recv3) = guest_conn3.accept_bi().await.expect("accept_bi");
        let new_handshake_res = CollabSessionStream::connect_guest_with_ticket(send3, recv3, &new_full_ticket).await;
        assert!(new_handshake_res.is_ok());

        server.stop().await;
        guest_ep.close().await;
    }

    #[tokio::test]
    async fn test_server_multiple_peers_and_update_snapshot() {
        let secret = CollabSecret::generate().expect("secret");
        let config = CollabHostConfig::new()
            .with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .with_secret(secret);

        let server = CollabHostServer::start(config).await.expect("start server");
        let (full_ticket, view_ticket) = server.tickets().await.expect("tickets");

        let guest_ep = Endpoint::builder(Minimal)
            .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("bind guest addr")
            .bind()
            .await
            .expect("guest bind");

        let conn1 = guest_ep
            .connect(server.endpoint().addr(), COLLAB_ALPN)
            .await
            .expect("connect 1");
        let (send1, recv1) = conn1.accept_bi().await.expect("accept_bi 1");
        let mut guest1 = CollabSessionStream::connect_guest_with_ticket(send1, recv1, &full_ticket)
            .await
            .expect("guest 1");
        let _ = guest1.recv_snapshot().await;

        let conn2 = guest_ep
            .connect(server.endpoint().addr(), COLLAB_ALPN)
            .await
            .expect("connect 2");
        let (send2, recv2) = conn2.accept_bi().await.expect("accept_bi 2");
        let mut guest2 = CollabSessionStream::connect_guest_with_ticket(send2, recv2, &view_ticket)
            .await
            .expect("guest 2");
        let _ = guest2.recv_snapshot().await;

        assert_eq!(server.peer_count().await, 2);

        server.broadcast(&RpcEvent::StatusChanged {
            status: "pairing".into(),
        });
        let ev1 = guest1.recv_event().await.expect("recv 1");
        let ev2 = guest2.recv_event().await.expect("recv 2");
        assert!(matches!(ev1, Some(RpcEvent::StatusChanged { status }) if status == "pairing"));
        assert!(matches!(ev2, Some(RpcEvent::StatusChanged { status }) if status == "pairing"));

        let new_snapshot = CollabSnapshot::new(
            vec![serde_json::json!({"turn": 2, "prompt": "turn 2 completed"})],
            "working",
        );
        server.update_snapshot(new_snapshot.clone()).await;

        let conn3 = guest_ep
            .connect(server.endpoint().addr(), COLLAB_ALPN)
            .await
            .expect("connect 3");
        let (send3, recv3) = conn3.accept_bi().await.expect("accept_bi 3");
        let mut guest3 = CollabSessionStream::connect_guest_with_ticket(send3, recv3, &full_ticket)
            .await
            .expect("guest 3");
        let snap3 = guest3.recv_snapshot().await.expect("recv snap 3");
        assert_eq!(snap3, Some(new_snapshot));
        assert_eq!(server.peer_count().await, 3);

        server.stop().await;
        guest_ep.close().await;
    }

    #[tokio::test]
    async fn test_server_peer_hostname_and_events() {
        let secret = CollabSecret::generate().expect("secret");
        let config = CollabHostConfig::new()
            .with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .with_secret(secret);

        let server = CollabHostServer::start(config).await.expect("start server");
        let (full_ticket, _) = server.tickets().await.expect("tickets");

        let guest_ep = Endpoint::builder(Minimal)
            .bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("bind guest addr")
            .bind()
            .await
            .expect("guest bind");

        let guest_conn = guest_ep
            .connect(server.endpoint().addr(), COLLAB_ALPN)
            .await
            .expect("guest connect");
        let (send, recv) = guest_conn.accept_bi().await.expect("accept_bi");

        let guest_stream = CollabSessionStream::connect_guest_with_ticket_and_hostname(
            send,
            recv,
            &full_ticket,
            Some("host-alice-m3".into()),
        )
        .await
        .expect("connect guest");

        let connect_ev = server.recv_peer_event().await;
        let peer_info = match connect_ev {
            Some(CollabPeerEvent::Connected(info)) => info,
            other => panic!("expected Connected, got {other:?}"),
        };
        assert_eq!(peer_info.id, 1);
        assert_eq!(peer_info.hostname.as_deref(), Some("host-alice-m3"));
        assert_eq!(peer_info.display_name(), "host-alice-m3");

        let peers = server.peers().await;
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].hostname.as_deref(), Some("host-alice-m3"));

        drop(guest_stream);
        let disconnect_ev = server.recv_peer_event().await;
        match disconnect_ev {
            Some(CollabPeerEvent::Disconnected {
                peer_id, display_name, ..
            }) => {
                assert_eq!(peer_id, 1);
                assert_eq!(display_name, "host-alice-m3");
            }
            other => panic!("expected Disconnected, got {other:?}"),
        }

        server.stop().await;
        guest_ep.close().await;
    }
}
