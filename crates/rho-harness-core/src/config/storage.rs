use crate::error::{AppError, Result};
use std::path::Path;
use std::str::FromStr;

use super::edit::update_config_file;
use super::scalar::{parse_bool, parse_positive, parse_retention_days};
use super::types::{
    BashConfigFile, ConfigKey, FileConfig, LspConfigFile, McpServerConfig, ModelRole, PermissionConfig,
    RecapConfigFile, UiConfig, WebFetchConfigFile, WebSearchConfigFile,
};
use super::validate::parse_search_engine;

impl super::Config {
    pub fn set_file_value(config_dir: &Path, key: &str, value: &str) -> Result<()> {
        let setting = Setting::parse(key)?;
        update_config_file(&config_dir.join("config.toml"), &setting.toml_keys(), |file_config| {
            setting.apply(file_config, value)
        })
    }

    pub async fn set_file_value_async(config_dir: &Path, key: &str, value: &str) -> Result<()> {
        let (config_dir, key, value) = (config_dir.to_path_buf(), key.to_string(), value.to_string());
        run_blocking(move || Self::set_file_value(&config_dir, &key, &value)).await
    }

    pub async fn save_default_model_async(config_dir: &Path, model: &str, provider: &str) -> Result<()> {
        let spec = if provider.trim().is_empty() {
            model.to_string()
        } else {
            crate::provider::ModelRoute::new(provider, model).to_string()
        };
        Self::set_file_value_async(config_dir, ConfigKey::Model.as_str(), &spec).await
    }

    pub async fn save_default_thinking_level_async(config_dir: &Path, thinking_level: Option<&str>) -> Result<()> {
        let path = config_dir.join("config.toml");
        let thinking_level = thinking_level.map(ToString::to_string);
        run_blocking(move || {
            let keys = Setting::Key(ConfigKey::ThinkingLevel).toml_keys();
            update_config_file(&path, &keys, |file_config| {
                file_config.thinking_level = thinking_level;
                Ok(())
            })
        })
        .await
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

async fn run_blocking(job: impl FnOnce() -> Result<()> + Send + 'static) -> Result<()> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|error| AppError::Config(format!("Failed to save config: {error}")))?
}

enum Setting {
    Role(ModelRole),
    Key(ConfigKey),
}

impl Setting {
    fn parse(key: &str) -> Result<Self> {
        if let Some(raw_role) = key.strip_prefix("models.")
            && raw_role != "default"
        {
            return ModelRole::parse(raw_role).map(Self::Role).map_err(AppError::Config);
        }
        ConfigKey::from_str(key).map(Self::Key).map_err(AppError::Config)
    }

    /// The dotted `config.toml` keys a write of this setting may change.
    fn toml_keys(&self) -> Vec<String> {
        match self {
            Self::Role(role) => vec![format!("models.{}", role.key())],
            Self::Key(ConfigKey::Model | ConfigKey::Provider) => {
                vec![
                    "models.default".to_string(),
                    "model".to_string(),
                    "provider".to_string(),
                ]
            }
            Self::Key(key) => vec![key.as_str().to_string()],
        }
    }

    fn apply(&self, file_config: &mut FileConfig, value: &str) -> Result<()> {
        match self {
            Self::Role(role) => {
                file_config.models.set_role(role, parse_optional_model(value));
                Ok(())
            }
            Self::Key(key) => apply_config_key(file_config, *key, value),
        }
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

/// The v0.10 top-level `model` and `provider` keys are kept on rewrite, but once `models.default`
/// is written they would only produce a conflict warning.
fn retire_legacy_model_keys(file_config: &mut FileConfig) {
    file_config.extra.remove("model");
    file_config.extra.remove("provider");
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

fn bash_section(file_config: &mut FileConfig) -> &mut BashConfigFile {
    let tools = file_config.tools.get_or_insert_with(Default::default);
    tools.bash.get_or_insert_with(Default::default)
}

fn recap_section(file_config: &mut FileConfig) -> &mut RecapConfigFile {
    file_config.recap.get_or_insert_with(Default::default)
}

fn permission_section(file_config: &mut FileConfig) -> &mut PermissionConfig {
    file_config.permission.get_or_insert_with(Default::default)
}

fn set_bool(slot: &mut bool, name: &str, value: &str) -> Result<()> {
    *slot = parse_bool(name, value)?;
    Ok(())
}

fn set_opt_bool(slot: &mut Option<bool>, name: &str, value: &str) -> Result<()> {
    *slot = Some(parse_bool(name, value)?);
    Ok(())
}

fn set_positive<T>(slot: &mut T, name: &str, value: &str) -> Result<()>
where
    T: FromStr + Default + PartialEq,
{
    *slot = parse_positive(name, value)?;
    Ok(())
}

fn set_opt_positive<T>(slot: &mut Option<T>, name: &str, value: &str) -> Result<()>
where
    T: FromStr + Default + PartialEq,
{
    *slot = Some(parse_positive(name, value)?);
    Ok(())
}

/// Every key is matched here, so adding a `ConfigKey` without a way to store it does not compile.
fn apply_config_key(file_config: &mut FileConfig, key: ConfigKey, value: &str) -> Result<()> {
    let name = key.as_str();
    match key {
        ConfigKey::Model => {
            retire_legacy_model_keys(file_config);
            file_config.models.default = Some(value.to_string());
        }
        ConfigKey::Provider => {
            let spec = default_model_on_provider(file_config, value)?;
            retire_legacy_model_keys(file_config);
            file_config.models.default = Some(spec);
        }
        ConfigKey::GuardModel => file_config
            .models
            .set_role(&ModelRole::GUARD, parse_optional_model(value)),
        ConfigKey::SmolModel => file_config
            .models
            .set_role(&ModelRole::SMOL, parse_optional_model(value)),
        ConfigKey::SlowModel => file_config
            .models
            .set_role(&ModelRole::SLOW, parse_optional_model(value)),
        ConfigKey::ThinkingLevel => {
            file_config.thinking_level =
                crate::thinking::ThinkingLevel::parse_setting(value).map_err(AppError::Config)?;
        }
        ConfigKey::Region => file_config.region = Some(value.to_string()),
        ConfigKey::SteeringMode => file_config.steering_mode = Some(value.parse().map_err(AppError::Config)?),
        ConfigKey::FollowUpMode => file_config.follow_up_mode = Some(value.parse().map_err(AppError::Config)?),
        ConfigKey::CacheRetention => file_config.cache_retention = Some(value.parse().map_err(AppError::Config)?),
        ConfigKey::ShowLabel => set_opt_bool(&mut file_config.show_label, name, value)?,
        ConfigKey::BlockStyle => ui_section(file_config).block_style = Some(value.to_string()),
        ConfigKey::AgentBlockOutput => set_opt_bool(&mut ui_section(file_config).agent_block_output, name, value)?,
        ConfigKey::HideThinking => set_opt_bool(&mut ui_section(file_config).hide_thinking, name, value)?,
        ConfigKey::ToolsExpanded => set_opt_bool(&mut ui_section(file_config).tools_expanded, name, value)?,
        ConfigKey::Cursor => ui_section(file_config).cursor = Some(value.to_string()),
        ConfigKey::SyntaxHighlightTools => {
            set_opt_bool(&mut ui_section(file_config).syntax_highlight_tools, name, value)?;
        }
        ConfigKey::FullBashCommand => set_opt_bool(&mut ui_section(file_config).full_bash_command, name, value)?,
        ConfigKey::StructuredToolCards => {
            set_opt_bool(&mut ui_section(file_config).structured_tool_cards, name, value)?;
        }
        ConfigKey::DeltaDiffs => set_opt_bool(&mut ui_section(file_config).delta_diffs, name, value)?,
        ConfigKey::AutoTitle => set_opt_bool(&mut ui_section(file_config).auto_title, name, value)?,
        ConfigKey::TerminalTitle => set_opt_bool(&mut ui_section(file_config).terminal_title, name, value)?,
        ConfigKey::TerminalTitleState => {
            set_opt_bool(&mut ui_section(file_config).terminal_title_state, name, value)?;
        }
        ConfigKey::A2uiWidgets => set_bool(&mut ui_section(file_config).a2ui_widgets, name, value)?,
        ConfigKey::A2uiMaxAboveLines => {
            set_positive(&mut ui_section(file_config).a2ui.max_above_lines, name, value)?;
        }
        ConfigKey::A2uiMaxBelowLines => {
            set_positive(&mut ui_section(file_config).a2ui.max_below_lines, name, value)?;
        }
        ConfigKey::A2uiTodoWidget => set_bool(&mut ui_section(file_config).a2ui.todo_widget, name, value)?,
        ConfigKey::A2uiSubagentWidget => set_bool(&mut ui_section(file_config).a2ui.subagent_widget, name, value)?,
        ConfigKey::A2uiDecisionAssist => set_bool(&mut ui_section(file_config).a2ui.decision_assist, name, value)?,
        ConfigKey::SearchEngine => {
            parse_search_engine(value)?;
            web_search_section(file_config).default = Some(value.to_string());
        }
        ConfigKey::WebSearchEnabled => set_opt_bool(&mut web_search_section(file_config).enabled, name, value)?,
        ConfigKey::WebFetchEnabled => set_opt_bool(&mut web_fetch_section(file_config).enabled, name, value)?,
        ConfigKey::LspEnabled => set_opt_bool(&mut lsp_section(file_config).enabled, name, value)?,
        ConfigKey::BashDirenv => set_opt_bool(&mut bash_section(file_config).direnv, name, value)?,
        ConfigKey::McpEnabled => {
            set_opt_bool(
                &mut file_config.mcp.get_or_insert_with(Default::default).enabled,
                name,
                value,
            )?;
        }
        ConfigKey::PermissionEnabled => set_bool(&mut permission_section(file_config).enabled, name, value)?,
        ConfigKey::GuardAllTools => set_bool(&mut permission_section(file_config).guard_all_tools, name, value)?,
        ConfigKey::GuardAuditLog => set_bool(&mut permission_section(file_config).guard_audit_log, name, value)?,
        ConfigKey::MaxOutputTokens => set_opt_positive(&mut file_config.max_output_tokens, name, value)?,
        ConfigKey::MaxTurns => set_opt_positive(&mut file_config.max_turns, name, value)?,
        ConfigKey::ContextLimit => set_opt_positive(&mut file_config.context_limit, name, value)?,
        ConfigKey::ContextWindowMessages => set_opt_positive(&mut file_config.context_window_messages, name, value)?,
        ConfigKey::CompactionMaxBytes => set_opt_positive(&mut file_config.compaction_max_bytes, name, value)?,
        ConfigKey::ReserveTokens => set_opt_positive(&mut file_config.reserve_tokens, name, value)?,
        ConfigKey::KeepRecentTokens => set_opt_positive(&mut file_config.keep_recent_tokens, name, value)?,
        ConfigKey::SearchMinIntervalMs => set_opt_positive(&mut file_config.search_min_interval_ms, name, value)?,
        ConfigKey::SearchTimeoutSec => set_opt_positive(&mut file_config.search_timeout_sec, name, value)?,
        ConfigKey::FetchTimeoutSec => set_opt_positive(&mut file_config.fetch_timeout_sec, name, value)?,
        ConfigKey::FetchLimit => set_opt_positive(&mut file_config.fetch_limit, name, value)?,
        ConfigKey::FetchMaxBytes => set_opt_positive(&mut file_config.fetch_max_bytes, name, value)?,
        ConfigKey::OutputMaxBytes => set_opt_positive(&mut file_config.output_max_bytes, name, value)?,
        ConfigKey::AllowPrivateNetwork => set_opt_bool(&mut file_config.allow_private_network, name, value)?,
        ConfigKey::SessionRetentionDays => {
            file_config.session_retention_days = Some(parse_retention_days(name, value)?);
        }
        ConfigKey::WorktreeBase => {
            file_config.worktree.get_or_insert_with(Default::default).base = Some(std::path::PathBuf::from(value));
        }
        ConfigKey::WorktreeCleanSource => {
            set_opt_bool(
                &mut file_config.worktree.get_or_insert_with(Default::default).clean_source,
                name,
                value,
            )?;
        }
        ConfigKey::RecapEnabled => set_opt_bool(&mut recap_section(file_config).enabled, name, value)?,
        ConfigKey::RecapIdleSeconds => set_opt_positive(&mut recap_section(file_config).idle_seconds, name, value)?,
        ConfigKey::RecapOnResume => set_opt_bool(&mut recap_section(file_config).on_resume, name, value)?,
    }
    Ok(())
}
