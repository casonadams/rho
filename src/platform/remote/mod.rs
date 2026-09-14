use anyhow::Result;
use rho_engine::auth::AuthStore;
use rho_harness_core::config::Config;
use std::sync::Arc;
use tokio::sync::OnceCell;

pub mod endpoint;
pub mod identity;
pub mod server;

static ACTIVE_REMOTE_URL: OnceCell<String> = OnceCell::const_new();

pub async fn ensure_remote_server(config: Config, auth_store: AuthStore) -> Result<String> {
    let url = ACTIVE_REMOTE_URL
        .get_or_try_init(|| async {
            let key_path = identity::default_secret_key_path()?;
            let secret = identity::load_or_generate_secret_key(&key_path)?;
            let (ws_listener, ws_port) = server::bind_ws_listener(None).await?;
            let endpoint = endpoint::RhoEndpoint::bind(secret, None).await?;
            let ticket = endpoint.ticket_with_ws(Some(ws_port))?;
            let pairing_url = endpoint::RhoEndpoint::pairing_url(&ticket);

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

            Ok::<String, anyhow::Error>(pairing_url)
        })
        .await?;
    Ok(url.clone())
}
