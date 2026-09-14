use anyhow::Result;
use rho_engine::auth::AuthStore;
use rho_harness_core::config::Config;
use rho_harness_core::presentation::InteractionResponse;
use rho_harness_core::rpc::protocol::RpcEvent;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{OnceCell, mpsc, oneshot};

pub mod endpoint;
pub mod identity;
pub mod server;

static REPL_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_repl_active(active: bool) {
    REPL_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn is_repl_active() -> bool {
    REPL_ACTIVE.load(Ordering::Relaxed)
}

pub type ActiveApprovalsMap = Arc<Mutex<HashMap<String, oneshot::Sender<InteractionResponse>>>>;

pub static ACTIVE_APPROVALS: std::sync::LazyLock<ActiveApprovalsMap> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

#[derive(Clone, Default)]
pub struct RemotePromptQueue {
    queue: Arc<Mutex<VecDeque<String>>>,
}

impl RemotePromptQueue {
    pub fn push(&self, prompt: String) {
        self.queue.lock().unwrap().push_back(prompt);
    }

    pub fn pop(&self) -> Option<String> {
        self.queue.lock().unwrap().pop_front()
    }
}

pub static REMOTE_PROMPT_QUEUE: std::sync::LazyLock<RemotePromptQueue> =
    std::sync::LazyLock::new(RemotePromptQueue::default);

#[derive(Clone, Default)]
pub struct PeerRegistry {
    peers: Arc<Mutex<Vec<mpsc::UnboundedSender<RpcEvent>>>>,
}

impl PeerRegistry {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn register(&self, tx: mpsc::UnboundedSender<RpcEvent>) {
        let mut list = self.peers.lock().unwrap();
        list.push(tx);
    }

    pub fn broadcast(&self, event: &RpcEvent) {
        let mut list = self.peers.lock().unwrap();
        list.retain(|tx| tx.send(event.clone()).is_ok());
    }

    pub fn peer_count(&self) -> usize {
        let mut list = self.peers.lock().unwrap();
        list.retain(|tx| !tx.is_closed());
        list.len()
    }
}

pub static PEER_REGISTRY: std::sync::LazyLock<PeerRegistry> = std::sync::LazyLock::new(PeerRegistry::new);

struct ActiveRemoteHandle {
    ticket: String,
}

static ACTIVE_REMOTE: OnceCell<ActiveRemoteHandle> = OnceCell::const_new();

pub async fn ensure_remote_server(config: Config, auth_store: AuthStore, session_id: Option<&str>) -> Result<String> {
    let handle = ACTIVE_REMOTE
        .get_or_try_init(|| async {
            let key_path = identity::default_secret_key_path()?;
            let secret = identity::load_or_generate_secret_key(&key_path)?;
            let (ws_listener, ws_port) = server::bind_ws_listener(None).await?;
            let endpoint = endpoint::RhoEndpoint::bind(secret, None).await?;
            let ticket = endpoint.ticket_with_ws(Some(ws_port))?;

            let base_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let base_dir = base_dir.canonicalize().unwrap_or(base_dir);
            let mut cfg = config.clone();
            cfg.sessions_dir = base_dir.join(".rho/sessions");

            let engine = crate::platform::agent_engine_in_dir(cfg.clone(), auth_store.clone(), base_dir, None).await?;
            let server = Arc::new(
                server::RemoteServer::new(endpoint.endpoint().clone(), cfg, auth_store, engine)
                    .with_ws_listener(ws_listener),
            );
            tokio::spawn(async move {
                let _ = server.run_accept_loop().await;
            });

            Ok::<ActiveRemoteHandle, anyhow::Error>(ActiveRemoteHandle { ticket })
        })
        .await?;

    Ok(endpoint::RhoEndpoint::pairing_url_with_session(
        &handle.ticket,
        session_id,
    ))
}
