use crate::error::{AppError, Result};
use std::path::Path;
use std::str::FromStr;

use super::types::{ConfigKey, FileConfig, McpServerConfig};

impl super::Config {
    pub fn set_file_value(config_dir: &Path, key: &str, value: &str) -> Result<()> {
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config(&path)?;
        apply_config_key(&mut file_config, key, value)?;
        write_file_config(&path, &file_config)
    }

    pub async fn set_file_value_async(config_dir: &Path, key: &str, value: &str) -> Result<()> {
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config_async(&path).await?;
        apply_config_key(&mut file_config, key, value)?;
        write_file_config_async(&path, &file_config).await
    }

    pub async fn save_default_model_async(config_dir: &Path, model: &str, provider: &str) -> Result<()> {
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config_async(&path).await?;
        file_config.model = Some(model.to_string());
        file_config.provider = Some(provider.to_string());
        file_config.models.insert(provider.to_string(), model.to_string());
        write_file_config_async(&path, &file_config).await
    }

    pub async fn save_default_thinking_level_async(config_dir: &Path, thinking_level: Option<&str>) -> Result<()> {
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config_async(&path).await?;
        file_config.thinking_level = thinking_level.map(ToString::to_string);
        write_file_config_async(&path, &file_config).await
    }

    pub async fn save_ui_block_style_async(config_dir: &Path, style: &str) -> Result<()> {
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config_async(&path).await?;
        let ui = file_config.ui.get_or_insert_with(Default::default);
        ui.block_style = Some(style.to_string());
        write_file_config_async(&path, &file_config).await
    }

    pub async fn save_ui_agent_box_async(config_dir: &Path, boxed: bool) -> Result<()> {
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config_async(&path).await?;
        let ui = file_config.ui.get_or_insert_with(Default::default);
        ui.agent_block_output = Some(boxed);
        write_file_config_async(&path, &file_config).await
    }

    pub async fn save_ui_hide_thinking_async(config_dir: &Path, hide: bool) -> Result<()> {
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config_async(&path).await?;
        let ui = file_config.ui.get_or_insert_with(Default::default);
        ui.hide_thinking = Some(hide);
        write_file_config_async(&path, &file_config).await
    }

    pub async fn save_ui_tools_expanded_async(config_dir: &Path, expanded: bool) -> Result<()> {
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config_async(&path).await?;
        let ui = file_config.ui.get_or_insert_with(Default::default);
        ui.tools_expanded = Some(expanded);
        write_file_config_async(&path, &file_config).await
    }

    pub async fn save_ui_cursor_async(config_dir: &Path, cursor: &str) -> Result<()> {
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config_async(&path).await?;
        let ui = file_config.ui.get_or_insert_with(Default::default);
        ui.cursor = Some(cursor.to_string());
        write_file_config_async(&path, &file_config).await
    }

    pub async fn save_show_label_async(config_dir: &Path, show: bool) -> Result<()> {
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config_async(&path).await?;
        file_config.show_label = Some(show);
        write_file_config_async(&path, &file_config).await
    }

    pub async fn save_semantic_search_async(config_dir: &Path, enabled: bool) -> Result<()> {
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config_async(&path).await?;
        file_config.semantic_search = Some(enabled);
        write_file_config_async(&path, &file_config).await
    }

    pub async fn save_default_search_engine_async(config_dir: &Path, engine: &str) -> Result<()> {
        Self::set_file_value_async(config_dir, "tools.web.search.default", engine).await
    }

    pub async fn save_web_search_enabled_async(config_dir: &Path, enabled: bool) -> Result<()> {
        Self::set_file_value_async(config_dir, "tools.web.search.enabled", &enabled.to_string()).await
    }

    pub async fn save_web_fetch_enabled_async(config_dir: &Path, enabled: bool) -> Result<()> {
        Self::set_file_value_async(config_dir, "tools.web.fetch.enabled", &enabled.to_string()).await
    }

    pub async fn save_mcp_enabled_async(config_dir: &Path, enabled: bool) -> Result<()> {
        Self::set_file_value_async(config_dir, "mcp.enabled", &enabled.to_string()).await
    }

    pub async fn save_permission_enabled_async(config_dir: &Path, enabled: bool) -> Result<()> {
        Self::set_file_value_async(config_dir, "permission.enabled", &enabled.to_string()).await
    }

    pub fn add_mcp_server(target: &Path, name: &str, server: McpServerConfig) -> Result<()> {
        let path = if target.is_dir() {
            super::mcp::global_mcp_path(target)
        } else {
            target.to_path_buf()
        };
        super::mcp::write_mcp_server_json(&path, name, &server)
    }

    pub fn remove_mcp_server(target: &Path, name: &str) -> Result<McpServerConfig> {
        let path = if target.is_dir() {
            let global = super::mcp::global_mcp_path(target);
            if global.is_file() {
                global
            } else {
                target.join("mcp.json")
            }
        } else {
            target.to_path_buf()
        };
        super::mcp::remove_mcp_server_json(&path, name)
    }
}

fn apply_provider_change(file_config: &mut FileConfig, value: &str) {
    let provider_changing = file_config.provider.as_deref().is_some_and(|p| p != value);
    file_config.provider = Some(value.to_string());
    if provider_changing {
        if let Some(model) = file_config.models.get(value) {
            file_config.model = Some(model.clone());
        } else {
            let default_m = crate::provider::default_model_for_provider(value);
            file_config.model = Some(default_m.to_string());
        }
    } else if let Some(m) = &file_config.model {
        file_config.models.insert(value.to_string(), m.clone());
    }
}

fn apply_model_key(file_config: &mut FileConfig, key: &ConfigKey, value: &str) -> Result<bool> {
    match key {
        ConfigKey::Model => {
            file_config.model = Some(value.to_string());
            if let Some(p) = &file_config.provider {
                file_config.models.insert(p.clone(), value.to_string());
            }
        }
        ConfigKey::Provider => apply_provider_change(file_config, value),
        ConfigKey::ThinkingLevel => {
            file_config.thinking_level = (value != "off").then(|| value.to_string());
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn apply_runtime_mode_key(file_config: &mut FileConfig, key: &ConfigKey, value: &str) -> Result<bool> {
    match key {
        ConfigKey::Region => file_config.region = Some(value.to_string()),
        ConfigKey::SteeringMode => file_config.steering_mode = Some(value.parse().map_err(AppError::Config)?),
        ConfigKey::FollowUpMode => file_config.follow_up_mode = Some(value.parse().map_err(AppError::Config)?),
        ConfigKey::ShowLabel => {
            file_config.show_label = Some(parse_bool(key.as_str(), value)?);
        }
        ConfigKey::SemanticSearch => {
            file_config.semantic_search = Some(parse_bool(key.as_str(), value)?);
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn apply_ui_key(file_config: &mut FileConfig, key: &ConfigKey, value: &str) -> Result<bool> {
    match key {
        ConfigKey::BlockStyle => {
            let ui = file_config.ui.get_or_insert_with(Default::default);
            ui.block_style = Some(value.to_string());
        }
        ConfigKey::AgentBlockOutput => {
            let ui = file_config.ui.get_or_insert_with(Default::default);
            ui.agent_block_output = Some(parse_bool(key.as_str(), value)?);
        }
        ConfigKey::HideThinking => {
            let ui = file_config.ui.get_or_insert_with(Default::default);
            ui.hide_thinking = Some(parse_bool(key.as_str(), value)?);
        }
        ConfigKey::ToolsExpanded => {
            let ui = file_config.ui.get_or_insert_with(Default::default);
            ui.tools_expanded = Some(parse_bool(key.as_str(), value)?);
        }
        ConfigKey::Cursor => {
            let ui = file_config.ui.get_or_insert_with(Default::default);
            ui.cursor = Some(value.to_string());
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn apply_tool_key(file_config: &mut FileConfig, key: &ConfigKey, value: &str) -> Result<bool> {
    match key {
        ConfigKey::SearchEngine => {
            if !crate::config::validate::is_valid_search_engine_name(value) {
                return Err(AppError::Config(format!(
                    "Unknown search engine '{value}'. Supported engines: brave, duckduckgo, yahoo, firecrawl, exa, gemini"
                )));
            }
            let tools = file_config.tools.get_or_insert_with(Default::default);
            let web = tools.web.get_or_insert_with(Default::default);
            let search = web.search.get_or_insert_with(Default::default);
            search.default = Some(value.to_string());
        }
        ConfigKey::WebSearchEnabled => {
            let tools = file_config.tools.get_or_insert_with(Default::default);
            let web = tools.web.get_or_insert_with(Default::default);
            let search = web.search.get_or_insert_with(Default::default);
            search.enabled = Some(parse_bool(key.as_str(), value)?);
        }
        ConfigKey::WebFetchEnabled => {
            let tools = file_config.tools.get_or_insert_with(Default::default);
            let web = tools.web.get_or_insert_with(Default::default);
            let fetch = web.fetch.get_or_insert_with(Default::default);
            fetch.enabled = Some(parse_bool(key.as_str(), value)?);
        }
        ConfigKey::McpEnabled => {
            let mcp = file_config.mcp.get_or_insert_with(Default::default);
            mcp.enabled = Some(parse_bool(key.as_str(), value)?);
        }
        ConfigKey::PermissionEnabled => {
            let permission = file_config.permission.get_or_insert_with(Default::default);
            permission.enabled = parse_bool(key.as_str(), value)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn apply_limit_key(file_config: &mut FileConfig, key: &ConfigKey, value: &str) -> Result<()> {
    match key {
        ConfigKey::MaxOutputTokens => file_config.max_output_tokens = Some(parse_positive(key.as_str(), value)?),
        ConfigKey::MaxTurns => file_config.max_turns = Some(parse_positive(key.as_str(), value)?),
        ConfigKey::ContextLimit => file_config.context_limit = Some(parse_positive(key.as_str(), value)?),
        ConfigKey::ContextWindowMessages => {
            file_config.context_window_messages = Some(parse_positive(key.as_str(), value)?)
        }
        ConfigKey::CompactionMaxBytes => file_config.compaction_max_bytes = Some(parse_positive(key.as_str(), value)?),
        ConfigKey::ReserveTokens => file_config.reserve_tokens = Some(parse_positive(key.as_str(), value)?),
        ConfigKey::KeepRecentTokens => file_config.keep_recent_tokens = Some(parse_positive(key.as_str(), value)?),
        _ => apply_net_key(file_config, key, value)?,
    }
    Ok(())
}

fn apply_net_key(file_config: &mut FileConfig, key: &ConfigKey, value: &str) -> Result<()> {
    if apply_fetch_search_key(file_config, key, value)? {
        return Ok(());
    }
    match key {
        ConfigKey::AllowPrivateNetwork => {
            file_config.allow_private_network = Some(parse_bool(key.as_str(), value)?);
        }
        ConfigKey::SessionRetentionDays => {
            file_config.session_retention_days = parse_retention(key.as_str(), value)?;
        }
        _ => {}
    }
    Ok(())
}

fn apply_fetch_search_key(file_config: &mut FileConfig, key: &ConfigKey, value: &str) -> Result<bool> {
    match key {
        ConfigKey::SearchMinIntervalMs => {
            file_config.search_min_interval_ms = Some(parse_positive(key.as_str(), value)?);
        }
        ConfigKey::SearchTimeoutSec => file_config.search_timeout_sec = Some(parse_positive(key.as_str(), value)?),
        ConfigKey::FetchTimeoutSec => file_config.fetch_timeout_sec = Some(parse_positive(key.as_str(), value)?),
        ConfigKey::FetchLimit => file_config.fetch_limit = Some(parse_positive(key.as_str(), value)?),
        ConfigKey::FetchMaxBytes => file_config.fetch_max_bytes = Some(parse_positive(key.as_str(), value)?),
        ConfigKey::OutputMaxBytes => file_config.output_max_bytes = Some(parse_positive(key.as_str(), value)?),
        _ => return Ok(false),
    }
    Ok(true)
}

fn parse_retention(key: &str, value: &str) -> Result<Option<u32>> {
    if value == "off" || value == "0" {
        Ok(Some(0))
    } else {
        Ok(Some(parse_positive(key, value)?))
    }
}

fn apply_config_key(file_config: &mut FileConfig, key: &str, value: &str) -> Result<()> {
    let key = ConfigKey::from_str(key).map_err(|error| AppError::Config(error.to_string()))?;
    if !apply_model_key(file_config, &key, value)?
        && !apply_runtime_mode_key(file_config, &key, value)?
        && !apply_ui_key(file_config, &key, value)?
        && !apply_tool_key(file_config, &key, value)?
    {
        apply_limit_key(file_config, &key, value)?;
    }
    Ok(())
}

fn read_file_config(path: &Path) -> Result<FileConfig> {
    if !path.exists() {
        return Ok(FileConfig::default());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|error| AppError::Config(format!("Failed to read config file {}: {error}", path.display())))?;
    toml::from_str(&content).map_err(|error| AppError::Config(format!("Failed to parse config file: {error}")))
}

async fn read_file_config_async(path: &Path) -> Result<FileConfig> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(FileConfig::default());
    }
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| AppError::Config(format!("Failed to read config file {}: {error}", path.display())))?;
    toml::from_str(&content).map_err(|error| AppError::Config(format!("Failed to parse config file: {error}")))
}

fn write_file_config(path: &Path, file_config: &FileConfig) -> Result<()> {
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

async fn atomic_replace_async(temporary: &Path, path: &Path) -> Result<()> {
    if let Err(error) = tokio::fs::rename(temporary, path).await {
        let _ = tokio::fs::remove_file(temporary).await;
        return Err(error.into());
    }
    Ok(())
}

async fn write_file_config_async(path: &Path, file_config: &FileConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let serialized = toml::to_string_pretty(file_config)
        .map_err(|error| AppError::Config(format!("Failed to serialize config: {error}")))?;
    let temporary = path.with_extension(format!("toml.{}.tmp", uuid::Uuid::new_v4()));
    tokio::fs::write(&temporary, serialized).await?;
    atomic_replace_async(&temporary, path).await
}

fn parse_bool(key: &str, value: &str) -> Result<bool> {
    value
        .parse()
        .map_err(|_| AppError::Config(format!("{key} must be true or false")))
}

fn parse_positive<T>(key: &str, value: &str) -> Result<T>
where
    T: FromStr + Default + PartialEq,
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
    fn apply_net_key_fallback_for_non_net_key() {
        let mut file_config = FileConfig::default();
        assert!(apply_net_key(&mut file_config, &ConfigKey::Model, "test").is_ok());
    }
}
