pub mod cli;

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub model: String,
    pub provider: String,
    pub auto_approve: bool,
    pub max_output_tokens: Option<u64>,
    pub max_turns: usize,
    pub context_limit: Option<usize>,
    pub context_window_messages: usize,
    pub compaction_max_bytes: usize,
    pub search_min_interval_ms: u64,
    pub search_timeout_sec: u64,
    pub search_total_timeout_sec: u64,
    pub fetch_timeout_sec: u64,
    pub fetch_limit: usize,
    pub fetch_max_bytes: usize,
    pub output_max_bytes: usize,
    pub allow_private_network: bool,
    pub region: String,
    pub config_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub auth_file: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        let base_dir = default_config_dir();
        Self {
            model: "claude-3-7-sonnet-20250219".to_string(),
            provider: "anthropic".to_string(),
            auto_approve: false,
            max_output_tokens: None,
            max_turns: 100,
            context_limit: None,
            context_window_messages: crate::session::context::DEFAULT_CONTEXT_WINDOW_MESSAGES,
            compaction_max_bytes: crate::session::context::DEFAULT_COMPACTION_MAX_BYTES,
            search_min_interval_ms: 2000,
            search_timeout_sec: 12,
            search_total_timeout_sec: 30,
            fetch_timeout_sec: 8,
            fetch_limit: 200,
            fetch_max_bytes: 5_000_000,
            output_max_bytes: 50_000,
            allow_private_network: false,
            region: "wt-wt".to_string(),
            sessions_dir: base_dir.join("sessions"),
            auth_file: base_dir.join("auth.json"),
            config_dir: base_dir,
        }
    }
}

pub fn default_config_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("RUST_AI_HOME") {
        return PathBuf::from(custom);
    }
    let home = dirs_fallback();
    home.join(".config").join("rust-ai")
}

fn dirs_fallback() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct FileConfig {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub auto_approve: Option<bool>,
    pub max_output_tokens: Option<u64>,
    pub max_turns: Option<usize>,
    pub context_limit: Option<usize>,
    pub context_window_messages: Option<usize>,
    pub compaction_max_bytes: Option<usize>,
    pub search_min_interval_ms: Option<u64>,
    pub search_timeout_sec: Option<u64>,
    pub fetch_timeout_sec: Option<u64>,
    pub fetch_limit: Option<usize>,
    pub fetch_max_bytes: Option<usize>,
    pub output_max_bytes: Option<usize>,
    pub allow_private_network: Option<bool>,
    pub region: Option<String>,
}

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
            config.merge_file(file_cfg);
        }

        // Environment overrides
        if let Ok(val) = std::env::var("AI_MODEL").or_else(|_| std::env::var("MODEL"))
            && !val.trim().is_empty()
        {
            config.model = val.trim().to_string();
        }
        if let Ok(val) = std::env::var("AI_PROVIDER")
            && !val.trim().is_empty()
        {
            config.provider = val.trim().to_string();
        }
        if let Ok(val) = std::env::var("AI_AUTO_APPROVE") {
            config.auto_approve = matches!(val.to_lowercase().as_str(), "1" | "true" | "yes");
        }
        if let Ok(val) = std::env::var("AI_CONTEXT_LIMIT")
            && let Ok(num) = val.trim().parse::<usize>()
        {
            config.context_limit = Some(num);
        }
        if let Ok(val) = std::env::var("AI_CONTEXT_WINDOW_MESSAGES") {
            config.context_window_messages = parse_positive("AI_CONTEXT_WINDOW_MESSAGES", &val)?;
        }
        if let Ok(val) = std::env::var("AI_COMPACTION_MAX_BYTES") {
            config.compaction_max_bytes = parse_positive("AI_COMPACTION_MAX_BYTES", &val)?;
        }
        if let Ok(val) = std::env::var("AI_MAX_OUTPUT_TOKENS") {
            config.max_output_tokens = Some(parse_positive("AI_MAX_OUTPUT_TOKENS", &val)?);
        }
        if let Ok(val) = std::env::var("AI_MAX_TURNS") {
            config.max_turns = parse_positive("AI_MAX_TURNS", &val)?;
        }
        if let Ok(val) = std::env::var("WEB_REGION") {
            config.region = val;
        }
        if let Ok(val) = std::env::var("WEB_ALLOW_PRIVATE_NETWORK") {
            config.allow_private_network = matches!(val.to_lowercase().as_str(), "1" | "true" | "yes");
        }

        // CLI flag overrides
        if let Some(c) = cli {
            if let Some(ref m) = c.model {
                config.model = m.clone();
            }
            if let Some(ref p) = c.provider {
                config.provider = p.clone();
            }
            if let Some(max_output_tokens) = c.max_output_tokens {
                config.max_output_tokens = Some(max_output_tokens);
            }
            if let Some(max_turns) = c.max_turns {
                config.max_turns = max_turns;
            }
            if c.auto_approve {
                config.auto_approve = true;
            }
        }

        config.validate()?;
        Ok(config)
    }

    fn merge_file(&mut self, file: FileConfig) {
        if let Some(m) = file.model {
            self.model = m;
        }
        if let Some(p) = file.provider {
            self.provider = p;
        }
        if let Some(a) = file.auto_approve {
            self.auto_approve = a;
        }
        if let Some(max_output_tokens) = file.max_output_tokens {
            self.max_output_tokens = Some(max_output_tokens);
        }
        if let Some(max_turns) = file.max_turns {
            self.max_turns = max_turns;
        }
        if let Some(c) = file.context_limit {
            self.context_limit = Some(c);
        }
        if let Some(value) = file.context_window_messages {
            self.context_window_messages = value;
        }
        if let Some(value) = file.compaction_max_bytes {
            self.compaction_max_bytes = value;
        }
        if let Some(s) = file.search_min_interval_ms {
            self.search_min_interval_ms = s;
        }
        if let Some(s) = file.search_timeout_sec {
            self.search_timeout_sec = s;
        }
        if let Some(f) = file.fetch_timeout_sec {
            self.fetch_timeout_sec = f;
        }
        if let Some(l) = file.fetch_limit {
            self.fetch_limit = l;
        }
        if let Some(b) = file.fetch_max_bytes {
            self.fetch_max_bytes = b;
        }
        if let Some(o) = file.output_max_bytes {
            self.output_max_bytes = o;
        }
        if let Some(p) = file.allow_private_network {
            self.allow_private_network = p;
        }
        if let Some(r) = file.region {
            self.region = r;
        }
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

fn parse_positive<T>(name: &str, value: &str) -> Result<T>
where
    T: std::str::FromStr + Default + PartialEq,
{
    let parsed = value
        .trim()
        .parse::<T>()
        .map_err(|_| AppError::Config(format!("{name} must be a positive integer")))?;
    if parsed == T::default() {
        return Err(AppError::Config(format!("{name} must be greater than zero")));
    }
    Ok(parsed)
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
        cfg.merge_file(file_cfg);
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
        assert_eq!(parse_positive::<usize>("LIMIT", "25").unwrap(), 25);
        assert!(parse_positive::<usize>("LIMIT", "0").is_err());
        assert!(parse_positive::<u64>("LIMIT", "invalid").is_err());
    }
}
