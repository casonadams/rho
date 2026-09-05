pub mod cli;
mod merge;
mod storage;
mod types;
mod validate;

#[cfg(test)]
mod tests;

pub use types::{
    Config, DEFAULT_MAX_TURNS, McpConfig, McpServerConfig, PermissionConfig, PluginConfig, ProviderConfig,
    default_config_dir, dirs_fallback,
};

use crate::error::{AppError, Result};
use types::FileConfig;

impl Config {
    pub fn load(cli: Option<&cli::Cli>) -> Result<Self> {
        let _ = dotenvy::dotenv();
        let mut config = Config::default();

        let config_file = config.config_dir.join("config.toml");
        if config_file.exists() {
            let content = std::fs::read_to_string(&config_file)
                .map_err(|e| AppError::Config(format!("Failed to read config file {}: {e}", config_file.display())))?;
            let file_cfg: FileConfig =
                toml::from_str(&content).map_err(|e| AppError::Config(format!("Failed to parse config file: {e}")))?;
            merge::merge_file(&mut config, file_cfg);
        }

        if let Ok(cwd) = std::env::current_dir()
            && cwd != types::dirs_fallback()
        {
            let project_config_file = cwd.join(".rho").join("config.toml");
            if project_config_file.exists()
                && let Ok(content) = std::fs::read_to_string(&project_config_file)
                && let Ok(project_file_cfg) = toml::from_str::<FileConfig>(&content)
            {
                merge::merge_file(&mut config, project_file_cfg);
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
