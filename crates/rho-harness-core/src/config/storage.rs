use crate::error::{AppError, Result};
use std::path::Path;
use std::str::FromStr;

use super::types::{
    ConfigKey, FileConfig, LspConfigFile, McpServerConfig, UiConfig, WebFetchConfigFile, WebSearchConfigFile,
};

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
        let spec = if provider.trim().is_empty() {
            model.to_string()
        } else {
            crate::provider::ModelRoute::new(provider, model).to_string()
        };
        file_config.models.default = Some(spec);
        write_file_config_async(&path, &file_config).await
    }

    pub async fn save_default_thinking_level_async(config_dir: &Path, thinking_level: Option<&str>) -> Result<()> {
        let path = config_dir.join("config.toml");
        let mut file_config = read_file_config_async(&path).await?;
        file_config.thinking_level = thinking_level.map(ToString::to_string);
        write_file_config_async(&path, &file_config).await
    }

    pub async fn save_ui_block_style_async(config_dir: &Path, style: &str) -> Result<()> {
        Self::set_file_value_async(config_dir, "ui.block_style", style).await
    }

    pub async fn save_ui_agent_box_async(config_dir: &Path, boxed: bool) -> Result<()> {
        Self::set_file_value_async(config_dir, "ui.agent_block_output", &boxed.to_string()).await
    }

    pub async fn save_ui_hide_thinking_async(config_dir: &Path, hide: bool) -> Result<()> {
        Self::set_file_value_async(config_dir, "ui.hide_thinking", &hide.to_string()).await
    }

    pub async fn save_ui_tools_expanded_async(config_dir: &Path, expanded: bool) -> Result<()> {
        Self::set_file_value_async(config_dir, "ui.tools_expanded", &expanded.to_string()).await
    }

    pub async fn save_ui_cursor_async(config_dir: &Path, cursor: &str) -> Result<()> {
        Self::set_file_value_async(config_dir, "ui.cursor", cursor).await
    }

    pub async fn save_show_label_async(config_dir: &Path, show: bool) -> Result<()> {
        Self::set_file_value_async(config_dir, "show_label", &show.to_string()).await
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

    pub fn add_mcp_server(config_dir: &Path, name: &str, server: McpServerConfig) -> Result<()> {
        super::mcp::write_mcp_server_json(&config_dir.join("mcp.json"), name, &server)
    }

    pub fn remove_mcp_server(config_dir: &Path, name: &str) -> Result<()> {
        super::mcp::remove_mcp_server_json(&config_dir.join("mcp.json"), name)
    }
}

fn parse_optional_model(value: &str) -> Option<String> {
    let trimmed = value.trim();
    match trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        true => None,
        false => Some(trimmed.to_string()),
    }
}

fn default_model_on_provider(file_config: &FileConfig, provider: &str) -> Result<String> {
    let current_spec = file_config.models.default.as_deref().ok_or_else(|| {
        AppError::Config(format!(
            "No default model to move to {provider}. Set one with: rho config model {provider}/<model>"
        ))
    })?;
    let route = crate::provider::ModelRoute::parse(current_spec, None, "local");
    Ok(format!("{provider}/{}", route.model))
}

fn ui_section(file_config: &mut FileConfig) -> &mut UiConfig {
    file_config.ui.get_or_insert_with(Default::default)
}

fn web_search_section(file_config: &mut FileConfig) -> &mut WebSearchConfigFile {
    let tools = file_config.tools.get_or_insert_with(Default::default);
    let web = tools.web.get_or_insert_with(Default::default);
    web.search.get_or_insert_with(Default::default)
}

fn web_fetch_section(file_config: &mut FileConfig) -> &mut WebFetchConfigFile {
    let tools = file_config.tools.get_or_insert_with(Default::default);
    let web = tools.web.get_or_insert_with(Default::default);
    web.fetch.get_or_insert_with(Default::default)
}

fn lsp_section(file_config: &mut FileConfig) -> &mut LspConfigFile {
    let tools = file_config.tools.get_or_insert_with(Default::default);
    tools.lsp.get_or_insert_with(Default::default)
}

/// Every key is matched here, so adding a `ConfigKey` without a way to store it does not compile.
fn apply_config_key(file_config: &mut FileConfig, key: &str, value: &str) -> Result<()> {
    let key = ConfigKey::from_str(key).map_err(|error| AppError::Config(error.to_string()))?;
    let name = key.as_str();
    match key {
        ConfigKey::Model => file_config.models.default = Some(value.to_string()),
        ConfigKey::Provider => file_config.models.default = Some(default_model_on_provider(file_config, value)?),
        ConfigKey::GuardModel => file_config.models.guard = parse_optional_model(value),
        ConfigKey::SmolModel => file_config.models.smol = parse_optional_model(value),
        ConfigKey::SlowModel => file_config.models.slow = parse_optional_model(value),
        ConfigKey::ThinkingLevel => {
            file_config.thinking_level =
                crate::thinking::ThinkingLevel::parse_setting(value).map_err(AppError::Config)?;
        }
        ConfigKey::Region => file_config.region = Some(value.to_string()),
        ConfigKey::SteeringMode => file_config.steering_mode = Some(value.parse().map_err(AppError::Config)?),
        ConfigKey::FollowUpMode => file_config.follow_up_mode = Some(value.parse().map_err(AppError::Config)?),
        ConfigKey::CacheRetention => file_config.cache_retention = Some(value.parse().map_err(AppError::Config)?),
        ConfigKey::ShowLabel => file_config.show_label = Some(parse_bool(name, value)?),
        ConfigKey::BlockStyle => ui_section(file_config).block_style = Some(value.to_string()),
        ConfigKey::AgentBlockOutput => ui_section(file_config).agent_block_output = Some(parse_bool(name, value)?),
        ConfigKey::HideThinking => ui_section(file_config).hide_thinking = Some(parse_bool(name, value)?),
        ConfigKey::ToolsExpanded => ui_section(file_config).tools_expanded = Some(parse_bool(name, value)?),
        ConfigKey::Cursor => ui_section(file_config).cursor = Some(value.to_string()),
        ConfigKey::SearchEngine => {
            if !crate::config::validate::is_valid_search_engine_name(value) {
                return Err(AppError::Config(format!(
                    "Unknown search engine '{value}'. Supported engines: {}",
                    crate::config::SUPPORTED_SEARCH_ENGINES
                )));
            }
            web_search_section(file_config).default = Some(value.to_string());
        }
        ConfigKey::WebSearchEnabled => web_search_section(file_config).enabled = Some(parse_bool(name, value)?),
        ConfigKey::WebFetchEnabled => web_fetch_section(file_config).enabled = Some(parse_bool(name, value)?),
        ConfigKey::LspEnabled => lsp_section(file_config).enabled = Some(parse_bool(name, value)?),
        ConfigKey::McpEnabled => {
            file_config.mcp.get_or_insert_with(Default::default).enabled = Some(parse_bool(name, value)?);
        }
        ConfigKey::PermissionEnabled => {
            file_config.permission.get_or_insert_with(Default::default).enabled = parse_bool(name, value)?;
        }
        ConfigKey::MaxOutputTokens => file_config.max_output_tokens = Some(parse_positive(name, value)?),
        ConfigKey::MaxTurns => file_config.max_turns = Some(parse_positive(name, value)?),
        ConfigKey::ContextLimit => file_config.context_limit = Some(parse_positive(name, value)?),
        ConfigKey::ContextWindowMessages => file_config.context_window_messages = Some(parse_positive(name, value)?),
        ConfigKey::CompactionMaxBytes => file_config.compaction_max_bytes = Some(parse_positive(name, value)?),
        ConfigKey::ReserveTokens => file_config.reserve_tokens = Some(parse_positive(name, value)?),
        ConfigKey::KeepRecentTokens => file_config.keep_recent_tokens = Some(parse_positive(name, value)?),
        ConfigKey::SearchMinIntervalMs => file_config.search_min_interval_ms = Some(parse_positive(name, value)?),
        ConfigKey::SearchTimeoutSec => file_config.search_timeout_sec = Some(parse_positive(name, value)?),
        ConfigKey::FetchTimeoutSec => file_config.fetch_timeout_sec = Some(parse_positive(name, value)?),
        ConfigKey::FetchLimit => file_config.fetch_limit = Some(parse_positive(name, value)?),
        ConfigKey::FetchMaxBytes => file_config.fetch_max_bytes = Some(parse_positive(name, value)?),
        ConfigKey::OutputMaxBytes => file_config.output_max_bytes = Some(parse_positive(name, value)?),
        ConfigKey::AllowPrivateNetwork => file_config.allow_private_network = Some(parse_bool(name, value)?),
        ConfigKey::SessionRetentionDays => file_config.session_retention_days = parse_retention(name, value)?,
    }
    Ok(())
}

fn parse_retention(key: &str, value: &str) -> Result<Option<u32>> {
    if value == "off" || value == "0" {
        Ok(Some(0))
    } else {
        Ok(Some(parse_positive(key, value)?))
    }
}

fn parse_file_config(content: &str) -> Result<FileConfig> {
    toml::from_str(content).map_err(|error| AppError::Config(format!("Failed to parse config file: {error}")))
}

fn read_error(path: &Path, error: std::io::Error) -> AppError {
    AppError::Config(format!("Failed to read config file {}: {error}", path.display()))
}

fn read_file_config(path: &Path) -> Result<FileConfig> {
    if !path.exists() {
        return Ok(FileConfig::default());
    }
    parse_file_config(&std::fs::read_to_string(path).map_err(|error| read_error(path, error))?)
}

async fn read_file_config_async(path: &Path) -> Result<FileConfig> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(FileConfig::default());
    }
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| read_error(path, error))?;
    parse_file_config(&content)
}

fn serialize_file_config(file_config: &FileConfig) -> Result<String> {
    toml::to_string_pretty(file_config)
        .map_err(|error| AppError::Config(format!("Failed to serialize config: {error}")))
}

fn temporary_path(path: &Path) -> std::path::PathBuf {
    path.with_extension(format!("toml.{}.tmp", uuid::Uuid::new_v4()))
}

fn write_file_config(path: &Path, file_config: &FileConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = temporary_path(path);
    std::fs::write(&temporary, serialize_file_config(file_config)?)?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

async fn write_file_config_async(path: &Path, file_config: &FileConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let temporary = temporary_path(path);
    tokio::fs::write(&temporary, serialize_file_config(file_config)?).await?;
    if let Err(error) = tokio::fs::rename(&temporary, path).await {
        let _ = tokio::fs::remove_file(&temporary).await;
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
