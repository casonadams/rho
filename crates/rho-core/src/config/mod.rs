pub mod cli;
mod merge;
mod types;

pub use types::{Config, McpConfig, McpServerConfig, PluginConfig, default_config_dir};

use crate::error::{AppError, Result};

use std::str::FromStr;

use types::{ConfigKey, FileConfig};

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

    fn validate(&self) -> Result<()> {
        if self.max_output_tokens == Some(0) {
            return Err(AppError::Config(
                "max_output_tokens must be greater than zero".to_string(),
            ));
        }
        if self.max_turns == 0 {
            return Err(AppError::Config("max_turns must be greater than zero".to_string()));
        }
        if self.context_window_messages == 0 {
            return Err(AppError::Config(
                "context_window_messages must be greater than zero".to_string(),
            ));
        }
        if self.compaction_max_bytes == 0 {
            return Err(AppError::Config(
                "compaction_max_bytes must be greater than zero".to_string(),
            ));
        }
        for (name, plugin) in &self.plugins {
            if !is_valid_plugin_name(name) {
                return Err(AppError::Config(format!("invalid plugin name '{name}'")));
            }
            if plugin.path.as_os_str().is_empty() {
                return Err(AppError::Config(format!("plugin '{name}' path must not be empty")));
            }
            if plugin.package.as_ref().is_some_and(|package| package.trim().is_empty()) {
                return Err(AppError::Config(format!("plugin '{name}' package must not be empty")));
            }
        }
        Ok(())
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.sessions_dir)?;
        Ok(())
    }

    pub fn set_file_value(config_dir: &std::path::Path, key: &str, value: &str) -> Result<()> {
        let path = config_dir.join("config.toml");
        let mut file_config = if path.exists() {
            let content = std::fs::read_to_string(&path)
                .map_err(|error| AppError::Config(format!("Failed to read config file {}: {error}", path.display())))?;
            toml::from_str::<FileConfig>(&content)
                .map_err(|error| AppError::Config(format!("Failed to parse config file: {error}")))?
        } else {
            FileConfig::default()
        };

        let key = ConfigKey::from_str(key).map_err(|error| AppError::Config(error.to_string()))?;
        match key {
            ConfigKey::Model => file_config.model = Some(value.to_string()),
            ConfigKey::Provider => file_config.provider = Some(value.to_string()),
            ConfigKey::AutoApprove => file_config.auto_approve = Some(parse_bool(key.as_str(), value)?),
            ConfigKey::MaxOutputTokens => file_config.max_output_tokens = Some(parse_positive(key.as_str(), value)?),
            ConfigKey::MaxTurns => file_config.max_turns = Some(parse_positive(key.as_str(), value)?),
            ConfigKey::ContextLimit => file_config.context_limit = Some(parse_positive(key.as_str(), value)?),
            ConfigKey::ContextWindowMessages => {
                file_config.context_window_messages = Some(parse_positive(key.as_str(), value)?)
            }
            ConfigKey::CompactionMaxBytes => {
                file_config.compaction_max_bytes = Some(parse_positive(key.as_str(), value)?)
            }
            ConfigKey::SearchMinIntervalMs => {
                file_config.search_min_interval_ms = Some(parse_positive(key.as_str(), value)?)
            }
            ConfigKey::SearchTimeoutSec => file_config.search_timeout_sec = Some(parse_positive(key.as_str(), value)?),
            ConfigKey::FetchTimeoutSec => file_config.fetch_timeout_sec = Some(parse_positive(key.as_str(), value)?),
            ConfigKey::FetchLimit => file_config.fetch_limit = Some(parse_positive(key.as_str(), value)?),
            ConfigKey::FetchMaxBytes => file_config.fetch_max_bytes = Some(parse_positive(key.as_str(), value)?),
            ConfigKey::OutputMaxBytes => file_config.output_max_bytes = Some(parse_positive(key.as_str(), value)?),
            ConfigKey::AllowPrivateNetwork => {
                file_config.allow_private_network = Some(parse_bool(key.as_str(), value)?)
            }
            ConfigKey::Region => file_config.region = Some(value.to_string()),
            ConfigKey::SteeringMode => file_config.steering_mode = Some(value.parse().map_err(AppError::Config)?),
            ConfigKey::FollowUpMode => file_config.follow_up_mode = Some(value.parse().map_err(AppError::Config)?),
            ConfigKey::ReserveTokens => file_config.reserve_tokens = Some(parse_positive(key.as_str(), value)?),
            ConfigKey::KeepRecentTokens => file_config.keep_recent_tokens = Some(parse_positive(key.as_str(), value)?),
            ConfigKey::ThinkingLevel => file_config.thinking_level = Some(value.to_string()),
        }

        write_file_config(&path, &file_config)
    }

    pub fn add_plugin(config_dir: &std::path::Path, name: &str, plugin: PluginConfig) -> Result<()> {
        if name.trim().is_empty() {
            return Err(AppError::Config("plugin name must not be empty".to_string()));
        }
        if plugin.path.as_os_str().is_empty() {
            return Err(AppError::Config("plugin path must not be empty".to_string()));
        }
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config(&path)?;
        file_config.plugins.insert(name.to_string(), plugin);
        write_file_config(&path, &file_config)
    }

    pub fn remove_plugin(config_dir: &std::path::Path, name: &str) -> Result<PluginConfig> {
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config(&path)?;
        let plugin = file_config
            .plugins
            .remove(name)
            .ok_or_else(|| AppError::Config(format!("plugin '{name}' is not configured")))?;
        write_file_config(&path, &file_config)?;
        Ok(plugin)
    }
}

fn is_valid_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-' || c == '_')
}

fn read_file_config(path: &std::path::Path) -> Result<FileConfig> {
    if !path.exists() {
        return Ok(FileConfig::default());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|error| AppError::Config(format!("Failed to read config file {}: {error}", path.display())))?;
    toml::from_str(&content).map_err(|error| AppError::Config(format!("Failed to parse config file: {error}")))
}

fn write_file_config(path: &std::path::Path, file_config: &FileConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let serialized = toml::to_string_pretty(file_config)
        .map_err(|error| AppError::Config(format!("Failed to serialize config: {error}")))?;
    let temporary = path.with_extension(format!("toml.{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, serialized)?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

fn parse_bool(key: &str, value: &str) -> Result<bool> {
    value
        .parse()
        .map_err(|_| AppError::Config(format!("{key} must be true or false")))
}

fn parse_positive<T>(key: &str, value: &str) -> Result<T>
where
    T: std::str::FromStr + Default + PartialEq,
{
    let parsed = value
        .parse::<T>()
        .map_err(|_| AppError::Config(format!("{key} must be a positive integer")))?;
    if parsed == T::default() {
        return Err(AppError::Config(format!("{key} must be a positive integer")));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests;
