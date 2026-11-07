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
    Config, DEFAULT_MAX_TURNS, McpConfig, McpDirectTools, McpExposureMode, McpLifecycleMode, McpServerConfig,
    McpTransportKind, ModelsConfig, PermissionConfig, ProviderConfig, ToolsConfig, UiConfig, WebFetchConfig,
    WebSearchConfig, WebToolsConfig, default_config_dir, default_idle_timeout_seconds, dirs_fallback,
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

        if let Ok(Some(global_mcp)) = mcp::load_global_mcp_config(&config.config_dir) {
            config.mcp.enabled = config.mcp.enabled && global_mcp.enabled;
            config.mcp.servers.extend(global_mcp.servers);
        }

        if let Ok(cwd) = std::env::current_dir()
            && cwd != types::dirs_fallback()
        {
            merge_project_config(&mut config, &cwd);
            match mcp::load_project_mcp_config(&cwd) {
                Ok(Some(project_mcp)) => {
                    config.mcp.enabled = config.mcp.enabled && project_mcp.enabled;
                    config.mcp.servers.extend(project_mcp.servers);
                }
                Ok(None) => {}
                Err(e) => config
                    .migration_warnings
                    .push(format!("Ignoring project MCP config in {}: {e}", cwd.display())),
            }
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
            .migration_warnings
            .push(format!("Ignoring project config {}: {e}", path.display())),
    }
}
