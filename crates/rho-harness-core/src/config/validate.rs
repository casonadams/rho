use crate::error::{AppError, Result};
use std::str::FromStr;

fn validate_limits(config: &super::Config) -> Result<()> {
    if config.max_output_tokens == Some(0) {
        return Err(AppError::Config(
            "max_output_tokens must be greater than zero".to_string(),
        ));
    }
    if config.max_turns == 0 {
        return Err(AppError::Config("max_turns must be greater than zero".to_string()));
    }
    if config.context_window_messages == 0 {
        return Err(AppError::Config(
            "context_window_messages must be greater than zero".to_string(),
        ));
    }
    if config.compaction_max_bytes == 0 {
        return Err(AppError::Config(
            "compaction_max_bytes must be greater than zero".to_string(),
        ));
    }
    Ok(())
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

pub(crate) fn is_valid_search_engine_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "brave"
            | "duckduckgo"
            | "ddg"
            | "ddg_lite"
            | "duckduckgo_lite"
            | "duckduckgolite"
            | "yahoo"
            | "firecrawl"
            | "exa"
            | "gemini"
            | "google"
    )
}

fn validate_search_engines(config: &super::types::WebSearchConfig) -> Result<()> {
    if !is_valid_search_engine_name(&config.default) {
        return Err(AppError::Config(format!(
            "Unknown search engine '{}'. Supported engines: brave, duckduckgo, yahoo, firecrawl, exa, gemini",
            config.default
        )));
    }
    for engine in &config.fallback {
        if !is_valid_search_engine_name(engine) {
            return Err(AppError::Config(format!(
                "Unknown search engine '{}'. Supported engines: brave, duckduckgo, yahoo, firecrawl, exa, gemini",
                engine
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
