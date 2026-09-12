use super::super::batch::LiveBatch;
use super::super::modal::ModalKeyResult;
use super::super::navigation::{hydrate_session_transcript, update_footer};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub(crate) struct ModalActionContext<'a, 'b, 'c, B: TerminalBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub history: &'a mut InteractiveHistory,
    pub session: &'b mut ReplSession,
    pub engine: &'c mut AgentEngine,
}

fn print_model_status(
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
    (model, provider, save_as_default): (&str, &str, bool),
) {
    if save_as_default {
        ctx.session.config.set_default_model(model, provider);
        ctx.session
            .renderer
            .print_status(&format!("Default model: {model} ({provider})"));
    } else {
        ctx.session
            .renderer
            .print_status(&format!("Model: {model} ({provider})"));
    }
}

async fn handle_model_selected(
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
    (model, provider, save_as_default): (String, String, bool),
    batch: &mut LiveBatch,
) -> Result<bool> {
    ctx.session.config.model = model.clone();
    ctx.session.config.provider = provider.clone();
    if save_as_default {
        let _ = rho_harness_core::config::Config::save_default_model_async(
            &ctx.session.config.config_dir,
            &model,
            &provider,
        )
        .await;
    }
    print_model_status(ctx, (&model, &provider, save_as_default));
    if let Err(err) = ctx.engine.switch_model(&model, &provider).await {
        ctx.session
            .renderer
            .print_notice(&format!("\nWarning: Could not switch model: {err}\n"));
    }
    update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
    batch.flush(ctx.controller, true)?;
    Ok(true)
}

async fn handle_node_selected(
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
    node_id: String,
) -> Result<bool> {
    match ctx.engine.session_manager.switch_branch(Some(node_id.clone())).await {
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

async fn handle_node_label_updated(
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
    (node_id, label): (String, String),
) -> Result<bool> {
    let label_opt = if label.is_empty() { None } else { Some(label.clone()) };
    match ctx.engine.session_manager.set_node_label(&node_id, label_opt).await {
        Ok(_) => ctx
            .session
            .renderer
            .print_status(&format!("Checkpoint labeled: \"{label}\" ({node_id})")),
        Err(err) => ctx
            .session
            .renderer
            .print_status(&format!("Failed to label checkpoint: {err}")),
    }
    ctx.controller.redraw()?;
    Ok(true)
}

async fn handle_session_selected(
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
    session_id: String,
) -> Result<bool> {
    *ctx.engine = crate::platform::agent_engine(
        ctx.session.config.clone(),
        ctx.session.auth_store.clone(),
        Some(&session_id),
    )
    .await?;
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
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
    level: Option<&str>,
    save_as_default: bool,
) {
    let display = level.unwrap_or("off");
    if save_as_default {
        let _ =
            rho_harness_core::config::Config::save_default_thinking_level_async(&ctx.session.config.config_dir, level)
                .await;
        ctx.session
            .renderer
            .print_status(&format!("Default thinking level: {display}"));
    } else {
        ctx.session.renderer.print_status(&format!("Thinking: {display}"));
    }
}

async fn handle_thinking_selected(
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
    (level, save_as_default): (Option<String>, bool),
) -> Result<bool> {
    ctx.session.config.thinking_level = level.clone();
    ctx.engine.config.thinking_level = level.clone();
    save_or_print_thinking(ctx, level.as_deref(), save_as_default).await;
    if let Err(err) = ctx.engine.update_model().await {
        ctx.session
            .renderer
            .print_notice(&format!("\nWarning: Could not update thinking level: {err}\n"));
    }
    update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
    ctx.controller.redraw()?;
    Ok(true)
}

async fn handle_session_deleted(
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
    session_id: String,
) -> Result<bool> {
    let _ = rho_harness_core::session::delete_session_async(&ctx.session.config.sessions_dir, &session_id).await;
    ctx.session
        .renderer
        .print_status(&format!("Deleted session {session_id}"));
    ctx.controller.redraw()?;
    Ok(true)
}

async fn dispatch_modal_result_rest(
    res: ModalKeyResult,
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
) -> Result<bool> {
    match res {
        ModalKeyResult::SessionSelected { session_id } => handle_session_selected(ctx, session_id).await,
        rest => dispatch_modal_result_rest2(rest, ctx).await,
    }
}

async fn dispatch_session_or_node_result(
    res: &ModalKeyResult,
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
) -> Result<Option<bool>> {
    match res {
        ModalKeyResult::NodeLabelUpdated { node_id, label } => Ok(Some(
            handle_node_label_updated(ctx, (node_id.clone(), label.clone())).await?,
        )),
        ModalKeyResult::SessionDeleted { session_id } => {
            Ok(Some(handle_session_deleted(ctx, session_id.clone()).await?))
        }
        _ => Ok(None),
    }
}

async fn dispatch_modal_result_rest2(
    res: ModalKeyResult,
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
) -> Result<bool> {
    if let Some(handled) = dispatch_session_or_node_result(&res, ctx).await? {
        return Ok(handled);
    }
    match res {
        ModalKeyResult::ThinkingLevelSelected { level, save_as_default } => {
            handle_thinking_selected(ctx, (level, save_as_default)).await
        }
        ModalKeyResult::LoginProviderSelected { provider } => handle_login_provider_selected(ctx, provider).await,
        ModalKeyResult::HelpCommandSelected { command } => handle_help_command_selected(ctx, &command).await,
        ModalKeyResult::OpenModelSelector { save_as_default } => {
            super::super::modal::open_model_selector_with_default(ctx.session, ctx.controller, save_as_default);
            ctx.controller.redraw()?;
            Ok(true)
        }
        ModalKeyResult::BlockStyleToggled { style } => {
            ctx.session.config.ui.block_style = Some(style.clone());
            let _ = rho_harness_core::config::Config::save_ui_block_style_async(&ctx.session.config.config_dir, &style)
                .await;
            Ok(true)
        }
        ModalKeyResult::AgentBoxToggled { boxed } => {
            ctx.session.config.ui.agent_block_output = Some(boxed);
            let _ =
                rho_harness_core::config::Config::save_ui_agent_box_async(&ctx.session.config.config_dir, boxed).await;
            Ok(true)
        }
        ModalKeyResult::ShowLabelToggled { shown } => {
            ctx.session.config.show_label = shown;
            let _ =
                rho_harness_core::config::Config::save_show_label_async(&ctx.session.config.config_dir, shown).await;
            update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
            ctx.controller.redraw()?;
            Ok(true)
        }
        ModalKeyResult::ThinkingOutputToggled { hidden } => {
            ctx.session.config.ui.hide_thinking = Some(hidden);
            let _ =
                rho_harness_core::config::Config::save_ui_hide_thinking_async(&ctx.session.config.config_dir, hidden)
                    .await;
            Ok(true)
        }
        ModalKeyResult::ToolOutputToggled { expanded } => {
            ctx.session.config.ui.tools_expanded = Some(expanded);
            let _ = rho_harness_core::config::Config::save_ui_tools_expanded_async(
                &ctx.session.config.config_dir,
                expanded,
            )
            .await;
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

async fn dispatch_modal_result(
    res: ModalKeyResult,
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
    batch: &mut LiveBatch,
) -> Result<bool> {
    match res {
        ModalKeyResult::Handled => Ok(true),
        ModalKeyResult::ModelSelected {
            model,
            provider,
            save_as_default,
        } => handle_model_selected(ctx, (model, provider, save_as_default), batch).await,
        ModalKeyResult::TreeNodeSelected { node_id } => handle_node_selected(ctx, node_id).await,
        rest => dispatch_modal_result_rest(rest, ctx).await,
    }
}

async fn handle_login_provider_selected(
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
    provider: String,
) -> Result<bool> {
    ctx.controller.suspend()?;
    let login_res = crate::cli::login_provider(Some(&provider), &ctx.session.config, &mut ctx.session.auth_store).await;
    ctx.controller.resume()?;
    match login_res {
        Ok(()) => {
            *ctx.engine = ctx
                .engine
                .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
                .await?;
        }
        Err(crate::error::AppError::Cancelled(_)) => {}
        Err(err) => {
            ctx.session.renderer.print_notice(&format!("  Login failed: {err}\n"));
        }
    }
    update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
    ctx.controller.redraw()?;
    Ok(true)
}

async fn handle_help_command_selected(
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
    command: &str,
) -> Result<bool> {
    match command {
        "/settings" => {
            super::super::modal::open_settings_selector(
                Some(&ctx.session.config.model),
                ctx.session.config.thinking_level.as_deref(),
                ctx.controller,
            );
        }
        "/model" => {
            super::super::modal::open_model_selector(ctx.session, ctx.controller);
        }
        "/resume" => {
            super::super::modal::open_session_selector(&ctx.session.config.sessions_dir, ctx.controller);
        }
        "/tree" => {
            if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
                super::super::modal::open_tree_selector(&tree, ctx.controller);
            }
        }
        "/mcp" => {
            super::super::modal::open_mcp_selector(ctx.session, ctx.controller);
        }
        "/login" => {
            super::super::modal::open_login_selector(ctx.session, ctx.controller);
        }
        _ => {}
    }
    ctx.controller.redraw()?;
    Ok(true)
}

pub(crate) async fn apply_modal_key_result<B: TerminalBackend>(
    res: ModalKeyResult,
    mut ctx: ModalActionContext<'_, '_, '_, B>,
    batch: &mut LiveBatch,
) -> Result<bool> {
    dispatch_modal_result(res, &mut ctx, batch).await
}
