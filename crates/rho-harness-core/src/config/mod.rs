pub mod cli;
mod merge;
mod storage;
mod types;
mod validate;

#[cfg(test)]
mod tests;

pub use types::{
    Config, DEFAULT_MAX_TURNS, McpConfig, McpServerConfig, PluginConfig, ProviderConfig, default_config_dir,
};

use crate::error::{AppError, Result};
use types::FileConfig;

impl Config {
    pub fn load(cli: Option<&cli::Cli>) -> Result<Self> {
        let _ = dotenvy::dotenv();
        let mut config = Config::default();

        let state = crate::state::AppState::load(&config.config_dir);
        if let Some(m) = state.last_model {
            config.model = m;
        }
        if let Some(p) = state.last_provider {
            config.provider = p;
        }
        if let Some(t) = state.last_thinking_level {
            config.thinking_level = Some(t);
        }

        let config_file = config.config_dir.join("config.toml");
        if config_file.exists() {
            let content = std::fs::read_to_string(&config_file)
                .map_err(|e| AppError::Config(format!("Failed to read config file {}: {e}", config_file.display())))?;
            let file_cfg: FileConfig =
                toml::from_str(&content).map_err(|e| AppError::Config(format!("Failed to parse config file: {e}")))?;
            merge::merge_file(&mut config, file_cfg);
        }

        if let Ok(cwd) = std::env::current_dir() {
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
}
