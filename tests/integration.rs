pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn temp_dir(name: &str) -> std::path::PathBuf {
    tempfile::Builder::new().prefix(name).tempdir().unwrap().keep()
}

pub(crate) fn write_script(path: &std::path::Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

pub(crate) fn mcp_config(
    workspace: &std::path::Path,
    servers: std::collections::BTreeMap<String, rho_harness_core::config::McpServerConfig>,
) -> rho_harness_core::config::Config {
    rho_harness_core::config::Config {
        mcp: rho_harness_core::config::McpConfig {
            enabled: true,
            defer_threshold: 10,
            idle_timeout_seconds: 600,
            servers,
        },
        config_dir: workspace.to_path_buf(),
        sessions_dir: workspace.join("sessions"),
        auth_file: workspace.join("auth.json"),
        ..rho_harness_core::config::Config::default()
    }
}

pub(crate) async fn build_engine(
    workspace: &std::path::Path,
    config: rho_harness_core::config::Config,
) -> rho_engine::engine::AgentEngine {
    let auth_store = rho_engine::auth::AuthStore::load(&config.auth_file).unwrap_or_default();
    rho_engine::engine::builder::AgentEngineBuilder::new(config, auth_store)
        .base_dir(workspace.to_path_buf())
        .build()
        .await
        .unwrap()
}

#[path = "custom_provider.rs"]
mod custom_provider;
#[path = "eval.rs"]
mod eval;
#[path = "headless_integration.rs"]
mod headless_integration;
#[path = "hook_integration.rs"]
mod hook_integration;
#[path = "live_antigravity.rs"]
mod live_antigravity;
#[path = "live_claude.rs"]
mod live_claude;
#[path = "mcp_direct_tools.rs"]
mod mcp_direct_tools;
#[path = "mcp_eager_loading.rs"]
mod mcp_eager_loading;
#[path = "mcp_integration.rs"]
mod mcp_integration;
#[path = "mcp_lazy_lifecycle.rs"]
mod mcp_lazy_lifecycle;
#[path = "permission_integration.rs"]
mod permission_integration;
#[path = "repeat_call.rs"]
mod repeat_call;
#[path = "token_estimation_integration.rs"]
mod token_estimation_integration;
