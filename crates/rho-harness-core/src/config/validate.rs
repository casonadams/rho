use crate::error::{AppError, Result};
use std::str::FromStr;

/// Keys that `rho config` and the environment overrides already reject at zero, because zero
/// would disable the feature outright (a zero timeout, byte limit or context size).
fn validate_limits(config: &super::Config) -> Result<()> {
    let limits: [(&str, Option<u64>); 14] = [
        ("max_output_tokens", config.max_output_tokens),
        ("max_turns", Some(config.max_turns as u64)),
        ("context_limit", config.context_limit.map(|v| v as u64)),
        ("context_window_messages", Some(config.context_window_messages as u64)),
        ("compaction_max_bytes", Some(config.compaction_max_bytes as u64)),
        ("reserve_tokens", Some(config.reserve_tokens as u64)),
        ("keep_recent_tokens", Some(config.keep_recent_tokens as u64)),
        ("search_timeout_sec", Some(config.search_timeout_sec)),
        ("fetch_timeout_sec", Some(config.fetch_timeout_sec)),
        ("fetch_limit", Some(config.fetch_limit as u64)),
        ("fetch_max_bytes", Some(config.fetch_max_bytes as u64)),
        ("output_max_bytes", Some(config.output_max_bytes as u64)),
        ("ui.a2ui.max_above_lines", Some(config.ui.a2ui.max_above_lines as u64)),
        ("ui.a2ui.max_below_lines", Some(config.ui.a2ui.max_below_lines as u64)),
    ];
    match limits.iter().find(|(_, value)| *value == Some(0)) {
        Some((name, _)) => Err(AppError::Config(format!("{name} must be greater than zero"))),
        None => Ok(()),
    }
}

fn validate_single_provider(name: &str, provider: &crate::config::types::ProviderConfig) -> Result<()> {
    if !is_valid_provider_name(name) {
        return Err(AppError::Config(format!("invalid provider name '{name}'")));
    }
    if crate::provider::ProviderId::from_str(name).is_ok() {
        return Err(AppError::Config(format!(
            "provider name '{name}' conflicts with a built-in provider"
        )));
    }
    let parsed = url::Url::parse(&provider.base_url)
        .map_err(|e| AppError::Config(format!("provider '{name}' has invalid base_url: {e}")))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(AppError::Config(format!(
            "provider '{name}' base_url must use http or https"
        )));
    }
    Ok(())
}

fn validate_providers(config: &super::Config) -> Result<()> {
    for (name, provider) in &config.providers {
        validate_single_provider(name, provider)?;
    }
    Ok(())
}

pub const SUPPORTED_SEARCH_ENGINES: &str = "brave, duckduckgo, yahoo, exa";

pub(crate) fn is_valid_search_engine_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "brave" | "duckduckgo" | "ddg" | "ddg_lite" | "duckduckgo_lite" | "duckduckgolite" | "yahoo" | "exa"
    )
}

fn validate_search_engines(config: &super::types::WebSearchConfig) -> Result<()> {
    if !is_valid_search_engine_name(&config.default) {
        return Err(AppError::Config(format!(
            "Unknown search engine '{}'. Supported engines: {SUPPORTED_SEARCH_ENGINES}",
            config.default
        )));
    }
    for engine in &config.fallback {
        if !is_valid_search_engine_name(engine) {
            return Err(AppError::Config(format!(
                "Unknown search engine '{engine}'. Supported engines: {SUPPORTED_SEARCH_ENGINES}"
            )));
        }
    }
    Ok(())
}

impl super::Config {
    pub(super) fn validate(&self) -> Result<()> {
        validate_limits(self)?;
        validate_providers(self)?;
        validate_search_engines(&self.tools.web.search)
    }
}

pub(super) fn is_valid_provider_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-' || c == '_')
}
