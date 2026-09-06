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

fn validate_single_plugin(name: &str, plugin: &crate::config::types::PluginConfig) -> Result<()> {
    if !is_valid_plugin_name(name) {
        return Err(AppError::Config(format!("invalid plugin name '{name}'")));
    }
    if plugin.path.as_os_str().is_empty() && plugin.command.is_none() {
        return Err(AppError::Config(format!(
            "plugin '{name}' must specify a path or command"
        )));
    }
    if plugin.package.as_ref().is_some_and(|package| package.trim().is_empty()) {
        return Err(AppError::Config(format!("plugin '{name}' package must not be empty")));
    }
    Ok(())
}

fn validate_plugins(config: &super::Config) -> Result<()> {
    for (name, plugin) in &config.plugins {
        validate_single_plugin(name, plugin)?;
    }
    Ok(())
}

fn validate_single_provider(name: &str, provider: &crate::config::types::ProviderConfig) -> Result<()> {
    if !is_valid_plugin_name(name) {
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

impl super::Config {
    pub(super) fn validate(&self) -> Result<()> {
        validate_limits(self)?;
        validate_plugins(self)?;
        validate_providers(self)
    }
}

pub(super) fn is_valid_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-' || c == '_')
}
