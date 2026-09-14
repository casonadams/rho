use crate::auth::AuthStore;
use crate::config::Config;
use crate::platform::agent_engine_in_dir;
use crate::platform::remote::endpoint::RhoEndpoint;
use crate::platform::remote::identity::{default_secret_key_path, load_or_generate_secret_key};
use crate::platform::remote::server::RemoteServer;
use std::path::PathBuf;

pub async fn handle_serve(
    workspace: Option<String>,
    port: Option<u16>,
    name: Option<String>,
    config: &Config,
    auth_store: &AuthStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let base_dir = match workspace {
        Some(ws) => {
            let p = PathBuf::from(ws);
            if p.is_absolute() {
                p
            } else {
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(p)
            }
        }
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let base_dir = base_dir.canonicalize().unwrap_or(base_dir);

    let mut cfg = config.clone();
    cfg.sessions_dir = base_dir.join(".rho/sessions");

    let key_path = default_secret_key_path()?;
    let secret = load_or_generate_secret_key(&key_path)?;

    let endpoint = RhoEndpoint::bind(secret, port).await?;
    let ticket = endpoint.ticket()?;
    let pairing_url = RhoEndpoint::pairing_url(&ticket);
    let qr = RhoEndpoint::render_qr(&pairing_url)?;

    let node_name = name.unwrap_or_else(|| {
        std::env::var("HOSTNAME")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "rho-node".to_string())
    });

    println!("==================================================");
    println!(" rho remote node: {node_name}");
    println!(" workspace:       {}", base_dir.display());
    println!(" endpoint id:     {}", endpoint.id());
    println!("==================================================");
    println!("\nPairing URL:\n{pairing_url}\n");
    println!("Scan QR code with mobile camera to connect:\n");
    println!("{qr}\n");
    println!("Listening for peer connections... (Press Ctrl+C to exit)");

    let engine = agent_engine_in_dir(cfg.clone(), auth_store.clone(), base_dir, None).await?;
    let server = RemoteServer::new(endpoint.endpoint().clone(), cfg, auth_store.clone(), engine);

    server.run_accept_loop().await?;
    Ok(())
}
