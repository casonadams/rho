use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::live::navigation::update_footer;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{TerminalBackend, TerminalController};
use rho_harness_core::config::ModelRole;

use super::ModalKeyResult;

pub(crate) fn report_save(renderer: &TerminalRenderer, what: &str, result: &Result<()>) -> bool {
    match result {
        Ok(()) => true,
        Err(err) => {
            renderer.notice(
                crate::ui::render::NoticeKind::Warn,
                &format!("Could not save {what}: {err}"),
            );
            false
        }
    }
}

pub(crate) fn handle_modal_menu_open<B: TerminalBackend>(
    session: &ReplSession,
    controller: &mut TerminalController<B>,
    res: &ModalKeyResult,
) -> Result<bool> {
    match res {
        ModalKeyResult::OpenModelSelector { save_as_default } => {
            super::open_model_selector_with_default(session, controller, *save_as_default);
        }
        ModalKeyResult::OpenModelsMenu => {
            super::open_models_selector(session, controller);
        }
        ModalKeyResult::OpenRoleModelSelector { role } => {
            super::open_role_model_selector(session, controller, role);
        }
        ModalKeyResult::OpenToolsMenu => {
            super::open_tools_selector(session, controller);
        }
        ModalKeyResult::OpenSearchEngineSelector => {
            super::open_search_engine_selector(session, controller);
        }
        _ => return Ok(false),
    }
    controller.redraw()?;
    Ok(true)
}

async fn update_tool_toggle(
    renderer: &TerminalRenderer,
    name: &str,
    enabled: bool,
    persist: impl std::future::Future<Output = Result<()>>,
) -> Result<bool> {
    let saved = persist.await;
    if report_save(renderer, &format!("{name} setting"), &saved) {
        let status = if enabled { "enabled" } else { "disabled" };
        renderer.print_status(&format!("{name} {status}"));
    }
    Ok(true)
}

pub(crate) async fn handle_tool_setting_toggled(session: &mut ReplSession, res: &ModalKeyResult) -> Result<bool> {
    let dir = session.config.config_dir.clone();
    match res {
        ModalKeyResult::WebSearchToggled { enabled } => {
            let enabled = *enabled;
            session.config.tools.web.search.enabled = enabled;
            update_tool_toggle(
                &session.renderer,
                "Web search",
                enabled,
                rho_harness_core::config::Config::save_web_search_enabled_async(&dir, enabled),
            )
            .await
        }
        ModalKeyResult::WebFetchToggled { enabled } => {
            let enabled = *enabled;
            session.config.tools.web.fetch.enabled = enabled;
            update_tool_toggle(
                &session.renderer,
                "Web fetch",
                enabled,
                rho_harness_core::config::Config::save_web_fetch_enabled_async(&dir, enabled),
            )
            .await
        }
        ModalKeyResult::McpToggled { enabled } => {
            let enabled = *enabled;
            session.config.mcp.enabled = enabled;
            update_tool_toggle(
                &session.renderer,
                "MCP",
                enabled,
                rho_harness_core::config::Config::save_mcp_enabled_async(&dir, enabled),
            )
            .await
        }
        ModalKeyResult::PermissionToggled { enabled } => {
            let enabled = *enabled;
            session.config.permission.enabled = enabled;
            update_tool_toggle(
                &session.renderer,
                "Permissions",
                enabled,
                rho_harness_core::config::Config::save_permission_enabled_async(&dir, enabled),
            )
            .await
        }
        _ => Ok(false),
    }
}

pub(crate) async fn handle_ui_setting_toggled<B: TerminalBackend>(
    session: &mut ReplSession,
    engine: &AgentEngine,
    controller: &mut TerminalController<B>,
    res: &ModalKeyResult,
) -> Result<bool> {
    let dir = session.config.config_dir.clone();
    match res {
        ModalKeyResult::BlockStyleToggled { style } => {
            session.config.ui.block_style = Some(style.clone());
            let saved = rho_harness_core::config::Config::save_ui_block_style_async(&dir, style).await;
            report_save(&session.renderer, "block style", &saved);
            Ok(true)
        }
        ModalKeyResult::AgentBoxToggled { boxed } => {
            session.config.ui.agent_block_output = Some(*boxed);
            let saved = rho_harness_core::config::Config::save_ui_agent_box_async(&dir, *boxed).await;
            report_save(&session.renderer, "agent box", &saved);
            Ok(true)
        }
        ModalKeyResult::ShowLabelToggled { shown } => {
            session.config.show_label = *shown;
            let saved = rho_harness_core::config::Config::save_show_label_async(&dir, *shown).await;
            report_save(&session.renderer, "label display", &saved);
            update_footer(controller.state_mut(), session, engine);
            controller.redraw()?;
            Ok(true)
        }
        ModalKeyResult::ThinkingOutputToggled { hidden } => {
            session.config.ui.hide_thinking = Some(*hidden);
            let saved = rho_harness_core::config::Config::save_ui_hide_thinking_async(&dir, *hidden).await;
            report_save(&session.renderer, "thinking visibility", &saved);
            Ok(true)
        }
        ModalKeyResult::ToolOutputToggled { expanded } => {
            session.config.ui.tools_expanded = Some(*expanded);
            let saved = rho_harness_core::config::Config::save_ui_tools_expanded_async(&dir, *expanded).await;
            report_save(&session.renderer, "tool output setting", &saved);
            Ok(true)
        }
        ModalKeyResult::CursorToggled { cursor } => {
            session.config.ui.cursor = Some(cursor.clone());
            let saved = rho_harness_core::config::Config::save_ui_cursor_async(&dir, cursor).await;
            report_save(&session.renderer, "cursor style", &saved);
            Ok(true)
        }
        ModalKeyResult::McpServerToggled { server } => {
            if let Some(cfg) = session.config.mcp.servers.get_mut(server) {
                cfg.enabled = !cfg.enabled;
                let status = if cfg.enabled { "enabled" } else { "disabled" };
                session
                    .renderer
                    .print_notice(&format!("\nMCP server '{server}' {status}.\n"));
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub(crate) async fn handle_search_engine_selected<B: TerminalBackend>(
    session: &mut ReplSession,
    controller: &mut TerminalController<B>,
    engine_name: &str,
) -> Result<bool> {
    session.config.tools.web.search.default = engine_name.to_string();
    super::update_tools_search_engine(controller, engine_name);
    let saved =
        rho_harness_core::config::Config::save_default_search_engine_async(&session.config.config_dir, engine_name)
            .await;
    if report_save(&session.renderer, "default search engine", &saved) && controller.state().active_modal().is_none() {
        session
            .renderer
            .print_status(&format!("Default search engine set to {engine_name}"));
    }
    controller.redraw()?;
    Ok(true)
}

pub(crate) async fn handle_role_model_selected<B: TerminalBackend>(
    session: &mut ReplSession,
    controller: &mut TerminalController<B>,
    role: &ModelRole,
    model: &str,
    provider: &str,
) -> Result<bool> {
    let spec = if model.eq_ignore_ascii_case("none") || provider.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(rho_harness_core::provider::ModelRoute::new(provider, model).to_string())
    };

    let key = role.key();
    match &spec {
        Some(spec) => session.config.model_roles.insert(key.to_string(), spec.clone()),
        None => session.config.model_roles.remove(key),
    };

    let save_val = spec.as_deref().unwrap_or("none");
    let saved = rho_harness_core::config::Config::set_file_value_async(
        &session.config.config_dir,
        &format!("models.{key}"),
        save_val,
    )
    .await;

    let display = spec.as_deref().unwrap_or("None");
    super::update_models_role_description(controller, Some(role), display);
    if report_save(&session.renderer, &format!("{key} model"), &saved) && controller.state().active_modal().is_none() {
        session
            .renderer
            .print_status(&format!("{} model set to {display}", role.title()));
    }
    controller.redraw()?;
    Ok(true)
}

pub(crate) async fn save_or_print_thinking(session: &ReplSession, level: Option<&str>, save_as_default: bool) {
    let display = level.unwrap_or("off");
    if save_as_default {
        let saved =
            rho_harness_core::config::Config::save_default_thinking_level_async(&session.config.config_dir, level)
                .await;
        if report_save(&session.renderer, "default thinking level", &saved) {
            session
                .renderer
                .print_status(&format!("Default thinking level: {display}"));
        }
    } else {
        session.renderer.print_status(&format!("Thinking: {display}"));
    }
}

/// Opens any modal that is safe to launch from `/help` both while idle and mid-turn.
pub(crate) fn open_shared_help_modal<B: TerminalBackend>(
    command: &str,
    session: &ReplSession,
    engine: &AgentEngine,
    controller: &mut TerminalController<B>,
) -> Result<bool> {
    match command {
        "/settings" | "/thinking" => super::open_settings_selector(
            Some(&session.config.canonical_model_spec()),
            session.config.guard_model(),
            session.config.thinking_level.as_deref(),
            controller,
        ),
        "/model" => super::open_model_selector(session, controller),
        "/models" => super::open_models_selector(session, controller),
        "/tools" => super::open_tools_selector(session, controller),
        "/agents" => {
            super::agents::open_agents_selector(controller, engine.agent_monitor())?;
            return Ok(true);
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// Applies modal results that only mutate `session.config` / UI state (safe in both idle and turn loops).
pub(crate) async fn apply_shared_modal_action<B: TerminalBackend>(
    session: &mut ReplSession,
    engine: &AgentEngine,
    controller: &mut TerminalController<B>,
    res: &ModalKeyResult,
) -> Result<bool> {
    if handle_modal_menu_open(session, controller, res)? {
        return Ok(true);
    }
    if handle_tool_setting_toggled(session, res).await? {
        return Ok(true);
    }
    if handle_ui_setting_toggled(session, engine, controller, res).await? {
        return Ok(true);
    }
    match res {
        ModalKeyResult::SearchEngineSelected { engine: name } => {
            handle_search_engine_selected(session, controller, name).await
        }
        ModalKeyResult::RoleModelSelected { role, model, provider } => {
            handle_role_model_selected(session, controller, role, model, provider).await
        }
        _ => Ok(false),
    }
}
