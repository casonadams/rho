//! Layered configuration: defaults → config file → environment → CLI flags,
//! with validation and atomic persistence for settings.

pub mod cli;
pub mod mcp;
mod merge;
mod storage;
mod types;
mod validate;

pub use validate::SUPPORTED_SEARCH_ENGINES;

#[cfg(test)]
mod tests;

pub use types::{
    ApprovalFocus, BashConfig, CacheRetention, Config, DEFAULT_MAX_TURNS, LspConfig, McpConfig, McpDirectTools,
    McpExposureMode, McpLifecycleMode, McpOAuthConfig, McpServerConfig, McpTransportKind, ModelSlot, ModelsConfig,
    PermissionConfig, ProjectMcpServers, ProviderConfig, SubagentsConfig, ToolsConfig, UiConfig, WebFetchConfig,
    WebSearchConfig, WebToolsConfig, config_dir, default_config_dir, default_idle_timeout_seconds, dirs_fallback,
    home_dir,
};

use crate::error::{AppError, Result};
use std::path::Path;
use types::FileConfig;

impl Config {
    pub fn load(cli: Option<&cli::Cli>) -> Result<Self> {
        let mut config = Config::default();
        let _ = dotenvy::from_path(config.config_dir.join(".env"));

        let config_file = config.config_dir.join("config.toml");
        if config_file.exists() {
            let content = std::fs::read_to_string(&config_file)
                .map_err(|e| AppError::Config(format!("Failed to read config file {}: {e}", config_file.display())))?;
            let file_cfg: FileConfig =
                toml::from_str(&content).map_err(|e| AppError::Config(format!("Failed to parse config file: {e}")))?;
            merge::merge_file(&mut config, file_cfg);
        }

        match mcp::load_global_mcp_config(&config.config_dir) {
            Ok(Some(global_mcp)) => config.mcp.servers.extend(global_mcp.servers),
            Ok(None) => {}
            Err(e) => config.config_warnings.push(format!("Ignoring global MCP config: {e}")),
        }

        if let Ok(cwd) = std::env::current_dir()
            && cwd != types::dirs_fallback()
        {
            merge_project_config(&mut config, &cwd);
            merge_project_mcp(&mut config, &cwd);
        }

        merge::apply_env_overrides(&mut config)?;
        merge::apply_cli_overrides(&mut config, cli);
        config.validate()?;
        Ok(config)
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.sessions_dir)?;
        Ok(())
    }

    pub async fn ensure_dirs_async(&self) -> Result<()> {
        tokio::fs::create_dir_all(&self.config_dir).await?;
        tokio::fs::create_dir_all(&self.sessions_dir).await?;
        Ok(())
    }
}

fn merge_project_config(config: &mut Config, workspace: &Path) {
    let path = workspace.join(".rho").join("config.toml");
    if !path.exists() {
        return;
    }
    let parsed = std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|content| toml::from_str::<FileConfig>(&content).map_err(|e| e.to_string()));
    match parsed {
        Ok(file) => merge::merge_project_file(config, &file),
        Err(e) => config
            .config_warnings
            .push(format!("Ignoring project config {}: {e}", path.display())),
    }
}

fn merge_project_mcp(config: &mut Config, workspace: &Path) {
    match mcp::load_project_mcp_config(workspace) {
        Ok(Some(project_mcp)) => {
            config.mcp.project = Some(ProjectMcpServers {
                workspace: workspace.to_path_buf(),
                names: project_mcp.servers.keys().cloned().collect(),
            });
            config.mcp.servers.extend(project_mcp.servers);
        }
        Ok(None) => {}
        Err(e) => config
            .config_warnings
            .push(format!("Ignoring project MCP config in {}: {e}", workspace.display())),
    }
}
