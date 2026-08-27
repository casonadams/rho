pub mod cli;
mod merge;
mod types;

pub use types::{Config, default_config_dir};

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
