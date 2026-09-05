pub mod antigravity;
pub mod auth;
pub mod claude;
pub mod engine;
pub mod mcp;
pub mod ollama;
pub mod plugin;
pub mod process;
pub mod provider;
pub mod repeat;
pub mod tools;

pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
