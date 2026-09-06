use super::super::Config;
use crate::error::{AppError, Result};

pub(crate) fn apply_env_overrides(config: &mut Config) -> Result<()> {
    apply_env_overrides_with(config, |name| std::env::var(name).ok())
}

fn apply_model_env_overrides<F: Fn(&str) -> Option<String>>(config: &mut Config, get: &F) {
    if let Some(val) = get("AI_MODEL").or_else(|| get("MODEL"))
        && !val.trim().is_empty()
    {
        config.model = val.trim().to_string();
    }
    if let Some(val) = get("AI_PROVIDER")
        && !val.trim().is_empty()
    {
        config.provider = val.trim().to_string();
    }
    if let Some(val) = get("AI_THINKING_LEVEL") {
        config.thinking_level = (val != "off").then_some(val);
    }
}

fn apply_context_env_overrides<F: Fn(&str) -> Option<String>>(config: &mut Config, get: &F) -> Result<()> {
    if let Some(val) = get("AI_CONTEXT_LIMIT") {
        config.context_limit = Some(parse_positive("AI_CONTEXT_LIMIT", &val)?);
    }
    if let Some(val) = get("AI_CONTEXT_WINDOW_MESSAGES") {
        config.context_window_messages = parse_positive("AI_CONTEXT_WINDOW_MESSAGES", &val)?;
    }
    if let Some(val) = get("AI_COMPACTION_MAX_BYTES") {
        config.compaction_max_bytes = parse_positive("AI_COMPACTION_MAX_BYTES", &val)?;
    }
    if let Some(val) = get("AI_RESERVE_TOKENS") {
        config.reserve_tokens = parse_positive("AI_RESERVE_TOKENS", &val)?;
    }
    Ok(())
}

fn apply_token_env_overrides<F: Fn(&str) -> Option<String>>(config: &mut Config, get: &F) -> Result<()> {
    if let Some(val) = get("AI_KEEP_RECENT_TOKENS") {
        config.keep_recent_tokens = parse_positive("AI_KEEP_RECENT_TOKENS", &val)?;
    }
    if let Some(val) = get("AI_MAX_OUTPUT_TOKENS") {
        config.max_output_tokens = Some(parse_positive("AI_MAX_OUTPUT_TOKENS", &val)?);
    }
    if let Some(val) = get("AI_MAX_TURNS") {
        config.max_turns = parse_positive("AI_MAX_TURNS", &val)?;
    }
    if let Some(val) = get("AI_CONTEXT_INJECTION_MAX_TOKENS") {
        config.context_injection_max_tokens = parse_positive("AI_CONTEXT_INJECTION_MAX_TOKENS", &val)?;
    }
    Ok(())
}

fn apply_retention_env_override<F: Fn(&str) -> Option<String>>(config: &mut Config, get: &F) -> Result<()> {
    let Some(val) = get("AI_SESSION_RETENTION_DAYS").or_else(|| get("RHO_SESSION_RETENTION_DAYS")) else {
        return Ok(());
    };
    config.session_retention_days = if val == "off" || val == "0" {
        None
    } else {
        Some(parse_positive("AI_SESSION_RETENTION_DAYS", &val)?)
    };
    Ok(())
}

fn apply_runtime_env_overrides<F: Fn(&str) -> Option<String>>(config: &mut Config, get: &F) -> Result<()> {
    if let Some(val) = get("WEB_REGION") {
        config.region = val;
    }
    if let Some(val) = get("WEB_ALLOW_PRIVATE_NETWORK") {
        config.allow_private_network = parse_bool("WEB_ALLOW_PRIVATE_NETWORK", &val)?;
    }
    if let Some(val) = get("AI_STEERING_MODE") {
        config.steering_mode = val.parse().map_err(AppError::Config)?;
    }
    if let Some(val) = get("AI_FOLLOW_UP_MODE") {
        config.follow_up_mode = val.parse().map_err(AppError::Config)?;
    }
    apply_retention_env_override(config, get)?;
    Ok(())
}

pub(crate) fn apply_env_overrides_with<F>(config: &mut Config, get: F) -> Result<()>
where
    F: Fn(&str) -> Option<String>,
{
    apply_model_env_overrides(config, &get);
    apply_context_env_overrides(config, &get)?;
    apply_token_env_overrides(config, &get)?;
    apply_runtime_env_overrides(config, &get)?;
    Ok(())
}

fn parse_bool(name: &str, value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Ok(true),
        "0" | "false" | "no" => Ok(false),
        _ => Err(AppError::Config(format!("{name} must be true or false"))),
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
pub(crate) fn parse_positive_for_test<T>(name: &str, value: &str) -> Result<T>
where
    T: std::str::FromStr + Default + PartialEq,
{
    parse_positive(name, value)
}
