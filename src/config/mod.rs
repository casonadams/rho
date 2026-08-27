pub mod cli;
mod merge;
mod types;

pub use types::{Config, default_config_dir};

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
        }

        let serialized = toml::to_string_pretty(&file_config)
            .map_err(|error| AppError::Config(format!("Failed to serialize config: {error}")))?;
        let temporary = path.with_extension(format!("toml.{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&temporary, serialized)?;
        if let Err(error) = std::fs::rename(&temporary, &path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.into());
        }
        Ok(())
    }
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
mod tests {
    use super::*;

    #[test]
    fn test_set_file_value_persists_and_validates() {
        let dir = std::env::temp_dir().join(format!("rho_config_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        Config::set_file_value(&dir, "model", "gpt-test").unwrap();
        Config::set_file_value(&dir, "max_turns", "7").unwrap();
        let content = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        let file: FileConfig = toml::from_str(&content).unwrap();
        assert_eq!(file.model.as_deref(), Some("gpt-test"));
        assert_eq!(file.max_turns, Some(7));
        assert!(Config::set_file_value(&dir, "max_turns", "0").is_err());
        assert!(Config::set_file_value(&dir, "unknown", "value").is_err());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert!(!cfg.model.is_empty());
        assert_eq!(cfg.search_min_interval_ms, 2000);
        assert_eq!(cfg.output_max_bytes, 50_000);
        assert_eq!(cfg.max_output_tokens, None);
        assert_eq!(cfg.max_turns, 100);
        assert_eq!(cfg.context_window_messages, 24);
        assert_eq!(cfg.compaction_max_bytes, 8192);
        assert!(!cfg.allow_private_network);
    }

    #[test]
    fn test_file_merge() {
        let mut cfg = Config::default();
        let file_cfg = FileConfig {
            model: Some("gpt-4o".to_string()),
            provider: Some("openai".to_string()),
            auto_approve: Some(true),
            max_output_tokens: Some(8192),
            max_turns: Some(10),
            context_limit: Some(65536),
            context_window_messages: Some(16),
            compaction_max_bytes: Some(4096),
            search_min_interval_ms: Some(3000),
            ..Default::default()
        };
        merge::merge_file(&mut cfg, file_cfg);
        assert_eq!(cfg.model, "gpt-4o");
        assert_eq!(cfg.provider, "openai");
        assert!(cfg.auto_approve);
        assert_eq!(cfg.max_output_tokens, Some(8192));
        assert_eq!(cfg.max_turns, 10);
        assert_eq!(cfg.context_limit, Some(65536));
        assert_eq!(cfg.context_window_messages, 16);
        assert_eq!(cfg.compaction_max_bytes, 4096);
        assert_eq!(cfg.search_min_interval_ms, 3000);
    }

    #[test]
    fn test_precedence_is_defaults_file_environment_then_cli() {
        let mut config = Config::default();
        merge::merge_file(
            &mut config,
            FileConfig {
                model: Some("file-model".to_string()),
                max_turns: Some(20),
                ..Default::default()
            },
        );

        let environment = std::collections::HashMap::from([("AI_MODEL", "environment-model"), ("AI_MAX_TURNS", "30")]);
        merge::apply_env_overrides_with(&mut config, |name| {
            environment.get(name).map(|value| (*value).to_string())
        })
        .unwrap();

        let cli = cli::Cli {
            prompt: None,
            model: Some("cli-model".to_string()),
            provider: None,
            max_output_tokens: None,
            max_turns: Some(40),
            auto_approve: false,
            resume: None,
            command: None,
        };
        merge::apply_cli_overrides(&mut config, Some(&cli));

        assert_eq!(config.model, "cli-model");
        assert_eq!(config.max_turns, 40);
    }
    #[test]
    fn test_invalid_environment_values_are_rejected() {
        let mut config = Config::default();
        let environment = std::collections::HashMap::from([("AI_CONTEXT_LIMIT", "not-a-number")]);
        let error = merge::apply_env_overrides_with(&mut config, |name| {
            environment.get(name).map(|value| (*value).to_string())
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("AI_CONTEXT_LIMIT"));

        let environment = std::collections::HashMap::from([("AI_AUTO_APPROVE", "sometimes")]);
        let error = merge::apply_env_overrides_with(&mut config, |name| {
            environment.get(name).map(|value| (*value).to_string())
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("AI_AUTO_APPROVE"));
    }
    #[test]
    fn test_runtime_limit_boundaries() {
        let mut cfg = Config {
            max_turns: 0,
            ..Config::default()
        };
        assert!(cfg.validate().is_err());

        cfg.max_turns = 1;
        cfg.max_output_tokens = Some(0);
        assert!(cfg.validate().is_err());

        cfg.max_output_tokens = Some(1);
        cfg.context_window_messages = 0;
        assert!(cfg.validate().is_err());

        cfg.context_window_messages = 1;
        cfg.compaction_max_bytes = 0;
        assert!(cfg.validate().is_err());

        cfg.compaction_max_bytes = 1;
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_positive_integer_parsing() {
        assert_eq!(merge::parse_positive_for_test::<usize>("LIMIT", "25").unwrap(), 25);
        assert!(merge::parse_positive_for_test::<usize>("LIMIT", "0").is_err());
        assert!(merge::parse_positive_for_test::<u64>("LIMIT", "invalid").is_err());
    }
}
