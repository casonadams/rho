use super::super::batch::LiveBatch;
use super::super::modal::ModalKeyResult;
use super::super::navigation::{hydrate_session_transcript, update_footer};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::input_reader::TerminalInputReader;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{TerminalBackend, TerminalController};
use rho_harness_core::config::ModelSlot;

pub(crate) struct ModalActionContext<'a, B: TerminalBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub history: &'a mut InteractiveHistory,
    pub session: &'a mut ReplSession,
    pub engine: &'a mut AgentEngine,
    pub input: &'a mut TerminalInputReader,
}

fn print_model_status(ctx: &mut ModalActionContext<'_, impl TerminalBackend>, saved_default: bool) {
    let label = if saved_default { "Default model" } else { "Model" };
    let spec = ctx.session.config.canonical_model_spec();
    ctx.session.renderer.print_status(&format!("{label}: {spec}"));
}

fn report_save<B: TerminalBackend>(
    ctx: &mut ModalActionContext<'_, B>,
    what: &str,
    result: &crate::error::Result<()>,
) -> bool {
    match result {
        Ok(()) => true,
        Err(err) => {
            ctx.session.renderer.notice(
                crate::ui::render::NoticeKind::Warn,
                &format!("Could not save {what}: {err}"),
            );
            false
        }
    }
}

async fn handle_model_selected(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    model: String,
    provider: String,
    save_as_default: bool,
    batch: &mut LiveBatch,
) -> Result<bool> {
    let config = &mut ctx.session.config;
    config.set_route(rho_harness_core::provider::ModelRoute::new(&provider, &model));
    let saved = if save_as_default {
        rho_harness_core::config::Config::save_default_model_async(&config.config_dir, &config.model, &config.provider)
            .await
    } else {
        Ok(())
    };
    let saved_default = save_as_default && report_save(ctx, "default model", &saved);
    if saved_default {
        ctx.session.config.default_model = Some(ctx.session.config.canonical_model_spec());
    }
    if ctx.controller.state().active_modal().is_none() {
        print_model_status(ctx, saved_default);
    }
    let result = ctx.engine.switch_model(&model, &provider).await;
    crate::repl::live::navigation::report_model_update(ctx.session, ctx.engine, result, "Could not switch model");
    super::super::modal::update_models_role_description(ctx.controller, None, &model);
    super::super::modal::update_settings_model_description(ctx.controller, &model);
    update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
    batch.flush(ctx.controller, true)?;
    ctx.controller.redraw()?;
    Ok(true)
}

async fn handle_node_selected(ctx: &mut ModalActionContext<'_, impl TerminalBackend>, node_id: String) -> Result<bool> {
    match ctx.engine.switch_branch(Some(node_id.clone())).await {
        Ok(_) => {
            if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
                let _ = hydrate_session_transcript(ctx.controller, &tree, ctx.history);
            }
            ctx.session
                .renderer
                .print_status(&format!("Navigated to checkpoint {node_id}"));
        }
        Err(err) => {
            ctx.session.renderer.print_status(&format!("Failed to navigate: {err}"));
        }
    }
    ctx.controller.redraw()?;
    Ok(true)
}

fn report_outcome<T, E>(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    result: std::result::Result<T, E>,
    success: String,
    failure: impl FnOnce(&E) -> String,
) -> Result<bool> {
    let message = match result {
        Ok(_) => success,
        Err(err) => failure(&err),
    };
    ctx.session.renderer.print_status(&message);
    ctx.controller.redraw()?;
    Ok(true)
}

async fn handle_node_label_updated(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    node_id: String,
    label: String,
) -> Result<bool> {
    let label_opt = if label.is_empty() { None } else { Some(label.clone()) };
    let result = ctx.engine.session_manager.set_node_label(&node_id, label_opt).await;
    report_outcome(
        ctx,
        result,
        format!("Checkpoint labeled: \"{label}\" ({node_id})"),
        |err| format!("Failed to label checkpoint: {err}"),
    )
}

async fn handle_session_selected(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    session_id: String,
) -> Result<bool> {
    match ctx.session.new_engine(Some(&session_id)).await {
        Ok(engine) => ctx.engine.switch_to(engine),
        Err(err) => {
            ctx.session
                .renderer
                .print_status(&format!("Failed to resume session {session_id}: {err}"));
            ctx.controller.redraw()?;
            return Ok(true);
        }
    }
    ctx.session.sync_engine_model(ctx.engine).await;
    if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
        let _ = hydrate_session_transcript(ctx.controller, &tree, ctx.history);
    }
    ctx.session
        .renderer
        .print_status(&format!("Resumed session {session_id}"));
    update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
    ctx.controller.redraw()?;
    Ok(true)
}

async fn save_or_print_thinking(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    level: Option<&str>,
    save_as_default: bool,
) {
    let display = level.unwrap_or("off");
    if save_as_default {
        let saved =
            rho_harness_core::config::Config::save_default_thinking_level_async(&ctx.session.config.config_dir, level)
                .await;
        if report_save(ctx, "default thinking level", &saved) {
            ctx.session
                .renderer
                .print_status(&format!("Default thinking level: {display}"));
        }
    } else {
        ctx.session.renderer.print_status(&format!("Thinking: {display}"));
    }
}

async fn handle_thinking_selected(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    level: Option<String>,
    save_as_default: bool,
) -> Result<bool> {
    let changed = ctx.engine.config.thinking_level != level;
    ctx.session.config.thinking_level = level.clone();
    ctx.engine.config.thinking_level = level.clone();
    save_or_print_thinking(ctx, level.as_deref(), save_as_default).await;
    if changed {
        let result = ctx.engine.update_model().await;
        crate::repl::live::navigation::report_model_update(
            ctx.session,
            ctx.engine,
            result,
            "Could not update thinking level",
        );
    }
    update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
    ctx.controller.redraw()?;
    Ok(true)
}

async fn handle_session_deleted(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    session_id: String,
) -> Result<bool> {
    let result = rho_harness_core::session::delete_session_async(&ctx.session.config.sessions_dir, &session_id).await;
    report_outcome(ctx, result, format!("Deleted session {session_id}"), |err| {
        format!("Failed to delete session {session_id}: {err}")
    })
}

async fn execute_suspended_login<B: TerminalBackend>(
    ctx: &mut ModalActionContext<'_, B>,
    provider: &str,
) -> Result<crate::error::Result<()>> {
    let mut paused = ctx.input.pause()?;
    paused.drain();
    ctx.controller.suspend()?;
    let res = crate::cli::login_provider(Some(provider), false, &ctx.session.config, &mut ctx.session.auth_store).await;
    let c_res = ctx.controller.resume();
    let i_res = paused.resume();
    ctx.input.drain();
    c_res?;
    i_res?;
    Ok(res)
}

async fn handle_login_result<B: TerminalBackend>(
    ctx: &mut ModalActionContext<'_, B>,
    login_res: crate::error::Result<()>,
) -> Result<()> {
    match login_res {
        Ok(()) => match ctx
            .engine
            .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
            .await
        {
            Ok(engine) => {
                *ctx.engine = engine;
                ctx.session.refresh_model_catalog();
                crate::repl::report_mcp_diagnostics(ctx.session, ctx.engine);
            }
            Err(err) => ctx
                .session
                .renderer
                .print_notice(&format!("  Could not reload engine: {err}\n")),
        },
        Err(crate::error::AppError::Cancelled(_)) => {}
        Err(err) => {
            ctx.session.renderer.print_notice(&format!("  Login failed: {err}\n"));
        }
    }
    Ok(())
}

async fn handle_login_provider_selected(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    provider: String,
) -> Result<bool> {
    if let Some(server) = provider.strip_prefix("mcp:") {
        let theme = ctx.session.renderer.theme.clone();
        let mut ui = crate::repl::live::mcp_login::McpLoginUi {
            controller: ctx.controller,
            input: ctx.input,
            theme: &theme,
            open_url: crate::repl::live::mcp_login::system_browser(),
        };
        let signed_in = crate::repl::live::mcp_login::run_mcp_login(&mut ui, server, &ctx.session.config).await;
        if signed_in.is_ok() {
            handle_login_result(ctx, Ok(())).await?;
        }
    } else {
        let login_res = execute_suspended_login(ctx, &provider).await?;
        handle_login_result(ctx, login_res).await?;
    }
    update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
    ctx.controller.redraw()?;
    Ok(true)
}

async fn open_help_modal(
    command: &str,
    session: &ReplSession,
    active_session_id: &str,
    controller: &mut TerminalController<impl TerminalBackend>,
) -> bool {
    match command {
        "/settings" => super::super::modal::open_settings_selector(
            Some(&session.config.canonical_model_spec()),
            session.config.guard_model(),
            session.config.thinking_level.as_deref(),
            controller,
        ),
        "/model" => super::super::modal::open_model_selector(session, controller),
        "/resume" => {
            super::super::modal::open_session_selector(&session.config.sessions_dir, active_session_id, controller)
                .await
        }
        "/mcp" => super::super::modal::open_mcp_selector(session, controller),
        "/login" => super::super::modal::open_login_selector(session, controller),
        _ => return false,
    }
    true
}

async fn handle_help_command_selected(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    command: &str,
) -> Result<bool> {
    if command == "/tree" {
        if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
            super::super::modal::open_tree_selector(&tree, ctx.controller);
        }
    } else {
        open_help_modal(
            command,
            ctx.session,
            &ctx.engine.session_manager.session_id,
            ctx.controller,
        )
        .await;
    }
    ctx.controller.redraw()?;
    Ok(true)
}

async fn handle_ui_setting_toggled(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    res: ModalKeyResult,
) -> Result<bool> {
    match res {
        ModalKeyResult::BlockStyleToggled { style } => {
            ctx.session.config.ui.block_style = Some(style.clone());
            let saved =
                rho_harness_core::config::Config::save_ui_block_style_async(&ctx.session.config.config_dir, &style)
                    .await;
            report_save(ctx, "block style", &saved);
            Ok(true)
        }
        ModalKeyResult::AgentBoxToggled { boxed } => {
            ctx.session.config.ui.agent_block_output = Some(boxed);
            let saved =
                rho_harness_core::config::Config::save_ui_agent_box_async(&ctx.session.config.config_dir, boxed).await;
            report_save(ctx, "agent box", &saved);
            Ok(true)
        }
        ModalKeyResult::ShowLabelToggled { shown } => {
            ctx.session.config.show_label = shown;
            let saved =
                rho_harness_core::config::Config::save_show_label_async(&ctx.session.config.config_dir, shown).await;
            report_save(ctx, "label display", &saved);
            update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
            ctx.controller.redraw()?;
            Ok(true)
        }
        ModalKeyResult::ThinkingOutputToggled { hidden } => {
            ctx.session.config.ui.hide_thinking = Some(hidden);
            let saved =
                rho_harness_core::config::Config::save_ui_hide_thinking_async(&ctx.session.config.config_dir, hidden)
                    .await;
            report_save(ctx, "thinking visibility", &saved);
            Ok(true)
        }
        ModalKeyResult::ToolOutputToggled { expanded } => {
            ctx.session.config.ui.tools_expanded = Some(expanded);
            let saved = rho_harness_core::config::Config::save_ui_tools_expanded_async(
                &ctx.session.config.config_dir,
                expanded,
            )
            .await;
            report_save(ctx, "tool output setting", &saved);
            Ok(true)
        }
        ModalKeyResult::CursorToggled { cursor } => {
            ctx.session.config.ui.cursor = Some(cursor.clone());
            let saved =
                rho_harness_core::config::Config::save_ui_cursor_async(&ctx.session.config.config_dir, &cursor).await;
            report_save(ctx, "cursor style", &saved);
            Ok(true)
        }
        ModalKeyResult::McpServerToggled { server } => {
            if let Some(cfg) = ctx.session.config.mcp.servers.get_mut(&server) {
                cfg.enabled = !cfg.enabled;
                let status = if cfg.enabled { "enabled" } else { "disabled" };
                ctx.session
                    .renderer
                    .print_notice(&format!("\nMCP server '{server}' {status}.\n"));
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

async fn update_tool_toggle(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    name: &str,
    enabled: bool,
    persist: impl std::future::Future<Output = crate::error::Result<()>>,
) -> Result<bool> {
    let saved = persist.await;
    if report_save(ctx, &format!("{name} setting"), &saved) {
        let status = if enabled { "enabled" } else { "disabled" };
        ctx.session.renderer.print_status(&format!("{name} {status}"));
    }
    Ok(true)
}

async fn handle_tool_setting_toggled(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    res: &ModalKeyResult,
) -> Result<bool> {
    let dir = ctx.session.config.config_dir.clone();
    match res {
        ModalKeyResult::WebSearchToggled { enabled } => {
            let enabled = *enabled;
            ctx.session.config.tools.web.search.enabled = enabled;
            ctx.engine.config.tools.web.search.enabled = enabled;
            update_tool_toggle(
                ctx,
                "Web search",
                enabled,
                rho_harness_core::config::Config::save_web_search_enabled_async(&dir, enabled),
            )
            .await
        }
        ModalKeyResult::WebFetchToggled { enabled } => {
            let enabled = *enabled;
            ctx.session.config.tools.web.fetch.enabled = enabled;
            ctx.engine.config.tools.web.fetch.enabled = enabled;
            update_tool_toggle(
                ctx,
                "Web fetch",
                enabled,
                rho_harness_core::config::Config::save_web_fetch_enabled_async(&dir, enabled),
            )
            .await
        }
        ModalKeyResult::McpToggled { enabled } => {
            let enabled = *enabled;
            ctx.session.config.mcp.enabled = enabled;
            ctx.engine.config.mcp.enabled = enabled;
            update_tool_toggle(
                ctx,
                "MCP",
                enabled,
                rho_harness_core::config::Config::save_mcp_enabled_async(&dir, enabled),
            )
            .await
        }
        ModalKeyResult::PermissionToggled { enabled } => {
            let enabled = *enabled;
            ctx.session.config.permission.enabled = enabled;
            ctx.engine.config.permission.enabled = enabled;
            update_tool_toggle(
                ctx,
                "Permissions",
                enabled,
                rho_harness_core::config::Config::save_permission_enabled_async(&dir, enabled),
            )
            .await
        }
        _ => Ok(false),
    }
}

fn handle_modal_menu_open(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    res: &ModalKeyResult,
) -> Result<bool> {
    match res {
        ModalKeyResult::OpenModelSelector { save_as_default } => {
            super::super::modal::open_model_selector_with_default(ctx.session, ctx.controller, *save_as_default);
        }
        ModalKeyResult::OpenModelsMenu => {
            super::super::modal::open_models_selector(ctx.session, ctx.controller);
        }
        ModalKeyResult::OpenSlotModelSelector { slot } => {
            super::super::modal::open_slot_model_selector(ctx.session, ctx.controller, *slot);
        }
        ModalKeyResult::OpenToolsMenu => {
            super::super::modal::open_tools_selector(ctx.session, ctx.controller);
        }
        ModalKeyResult::OpenSearchEngineSelector => {
            super::super::modal::open_search_engine_selector(ctx.session, ctx.controller);
        }
        _ => return Ok(false),
    }
    ctx.controller.redraw()?;
    Ok(true)
}

async fn handle_search_engine_selected(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    engine: String,
) -> Result<bool> {
    ctx.session.config.tools.web.search.default = engine.clone();
    ctx.engine.config.tools.web.search.default = engine.clone();
    super::super::modal::update_tools_search_engine(ctx.controller, &engine);
    let saved =
        rho_harness_core::config::Config::save_default_search_engine_async(&ctx.session.config.config_dir, &engine)
            .await;
    if report_save(ctx, "default search engine", &saved) && ctx.controller.state().active_modal().is_none() {
        ctx.session
            .renderer
            .print_status(&format!("Default search engine set to {engine}"));
    }
    ctx.controller.redraw()?;
    Ok(true)
}

async fn handle_slot_model_selected(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    slot: ModelSlot,
    model: String,
    provider: String,
) -> Result<bool> {
    let spec = if model.eq_ignore_ascii_case("none") || provider.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(rho_harness_core::provider::ModelRoute::new(&provider, &model).to_string())
    };

    let key = slot.key();
    for config in [&mut ctx.session.config, &mut ctx.engine.config] {
        match &spec {
            Some(spec) => config.models.insert(key.to_string(), spec.clone()),
            None => config.models.remove(key),
        };
    }

    let save_val = spec.as_deref().unwrap_or("none");
    let saved = rho_harness_core::config::Config::set_file_value_async(
        &ctx.session.config.config_dir,
        &format!("models.{key}"),
        save_val,
    )
    .await;

    let display = spec.as_deref().unwrap_or("None");
    super::super::modal::update_models_role_description(ctx.controller, Some(slot), display);
    if report_save(ctx, &format!("{key} model"), &saved) && ctx.controller.state().active_modal().is_none() {
        ctx.session
            .renderer
            .print_status(&format!("{} model set to {display}", slot.title()));
    }
    ctx.controller.redraw()?;
    Ok(true)
}

async fn handle_session_modal_action(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    res: ModalKeyResult,
) -> Result<bool> {
    match res {
        ModalKeyResult::TreeNodeSelected { node_id } => handle_node_selected(ctx, node_id).await,
        ModalKeyResult::NodeLabelUpdated { node_id, label } => handle_node_label_updated(ctx, node_id, label).await,
        ModalKeyResult::SessionSelected { session_id } => handle_session_selected(ctx, session_id).await,
        ModalKeyResult::SessionDeleted { session_id } => handle_session_deleted(ctx, session_id).await,
        _ => Ok(false),
    }
}

async fn handle_selection_action(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    res: ModalKeyResult,
    batch: &mut LiveBatch,
) -> Result<bool> {
    match res {
        ModalKeyResult::ModelSelected {
            model,
            provider,
            save_as_default,
        } => handle_model_selected(ctx, model, provider, save_as_default, batch).await,
        ModalKeyResult::SlotModelSelected { slot, model, provider } => {
            handle_slot_model_selected(ctx, slot, model, provider).await
        }
        ModalKeyResult::ThinkingLevelSelected { level, save_as_default } => {
            handle_thinking_selected(ctx, level, save_as_default).await
        }
        ModalKeyResult::LoginProviderSelected { provider } => handle_login_provider_selected(ctx, provider).await,
        ModalKeyResult::HelpCommandSelected { command } => handle_help_command_selected(ctx, &command).await,
        _ => Ok(false),
    }
}

async fn dispatch_modal_result(
    res: ModalKeyResult,
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    batch: &mut LiveBatch,
) -> Result<bool> {
    match res {
        ModalKeyResult::Handled => Ok(true),
        ModalKeyResult::NotHandled => Ok(false),
        ModalKeyResult::SearchEngineSelected { engine } => handle_search_engine_selected(ctx, engine).await,
        action => {
            if handle_modal_menu_open(ctx, &action)? {
                return Ok(true);
            }
            if handle_tool_setting_toggled(ctx, &action).await? {
                return Ok(true);
            }
            if handle_session_modal_action(ctx, action.clone()).await? {
                return Ok(true);
            }
            if handle_selection_action(ctx, action.clone(), batch).await? {
                return Ok(true);
            }
            handle_ui_setting_toggled(ctx, action).await
        }
    }
}

pub(crate) async fn apply_modal_key_result<B: TerminalBackend>(
    res: ModalKeyResult,
    mut ctx: ModalActionContext<'_, B>,
    batch: &mut LiveBatch,
) -> Result<bool> {
    dispatch_modal_result(res, &mut ctx, batch).await
}
