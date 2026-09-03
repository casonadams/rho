use super::Config;
use crate::config::cli::Cli;
use crate::error::{AppError, Result};

pub(super) fn merge_file(config: &mut Config, file: super::FileConfig) {
    if let Some(m) = file.model {
        config.model = m;
    }
    if let Some(p) = file.provider {
        config.provider = p;
    }
    if let Some(a) = file.auto_approve {
        config.auto_approve = a;
    }
    if let Some(max_output_tokens) = file.max_output_tokens {
        config.max_output_tokens = Some(max_output_tokens);
    }
    if let Some(max_turns) = file.max_turns {
        config.max_turns = max_turns;
    }
    if let Some(c) = file.context_limit {
        config.context_limit = Some(c);
    }
    if let Some(value) = file.context_window_messages {
        config.context_window_messages = value;
    }
    if let Some(value) = file.compaction_max_bytes {
        config.compaction_max_bytes = value;
    }
    if let Some(value) = file.reserve_tokens {
        config.reserve_tokens = value;
    }
    if let Some(value) = file.keep_recent_tokens {
        config.keep_recent_tokens = value;
    }
    if let Some(s) = file.search_min_interval_ms {
        config.search_min_interval_ms = s;
    }
    if let Some(s) = file.search_timeout_sec {
        config.search_timeout_sec = s;
    }
    if let Some(f) = file.fetch_timeout_sec {
        config.fetch_timeout_sec = f;
    }
    if let Some(l) = file.fetch_limit {
        config.fetch_limit = l;
    }
    if let Some(b) = file.fetch_max_bytes {
        config.fetch_max_bytes = b;
    }
    if let Some(o) = file.output_max_bytes {
        config.output_max_bytes = o;
    }
    if let Some(p) = file.allow_private_network {
        config.allow_private_network = p;
    }
    if let Some(r) = file.region {
        config.region = r;
    }
    if let Some(v) = file.show_label {
        config.show_label = v;
    }
    if let Some(v) = file.show_version {
        config.show_version = v;
    }
    if let Some(s) = file.steering_mode {
        config.steering_mode = s;
    }
    if let Some(f) = file.follow_up_mode {
        config.follow_up_mode = f;
    }
    if let Some(t) = file.thinking_level {
        config.thinking_level = Some(t);
    }
    if let Some(tokens) = file.context_injection_max_tokens {
        config.context_injection_max_tokens = tokens;
    }
    if let Some(mcp) = file.mcp {
        config.mcp = mcp;
    }
    config.plugins = file.plugins;
    config.providers = file.providers;
}

pub(super) fn apply_env_overrides(config: &mut Config) -> Result<()> {
    apply_env_overrides_with(config, |name| std::env::var(name).ok())
}

pub(super) fn apply_env_overrides_with<F>(config: &mut Config, get: F) -> Result<()>
where
    F: Fn(&str) -> Option<String>,
{
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
    if let Some(val) = get("AI_AUTO_APPROVE") {
        config.auto_approve = parse_bool("AI_AUTO_APPROVE", &val)?;
    }
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
    if let Some(val) = get("AI_THINKING_LEVEL") {
        config.thinking_level = if val == "off" { None } else { Some(val) };
    }
    Ok(())
}

pub(super) fn apply_cli_overrides(config: &mut Config, cli: Option<&Cli>) {
    let Some(c) = cli else {
        return;
    };
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
    if let Some(ref t) = c.thinking {
        config.thinking_level = if t == "off" { None } else { Some(t.clone()) };
    }
    if c.auto_approve {
        config.auto_approve = true;
    }
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
pub(super) fn parse_positive_for_test<T>(name: &str, value: &str) -> Result<T>
where
    T: std::str::FromStr + Default + PartialEq,
{
    parse_positive(name, value)
}
