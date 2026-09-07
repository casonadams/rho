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

async fn handle_theme_selected(
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
    theme: String,
) -> Result<bool> {
    let registry = crate::ui::theme::ThemeRegistry::new(Some(&ctx.session.config.config_dir));
    if let Some(resolved) = registry.get(&theme).cloned() {
        ctx.session.config.theme = theme.clone();
        ctx.session.renderer.theme = resolved.clone();
        let _ = ctx.controller.set_theme(resolved);
        let _ = rho_harness_core::config::Config::set_file_value_async(&ctx.session.config.config_dir, "theme", &theme)
            .await;
        ctx.session.renderer.print_status(&format!("Theme: {theme}"));
    }
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

async fn handle_theme_or_thinking_selected(
    res: &ModalKeyResult,
    ctx: &mut ModalActionContext<'_, '_, '_, impl TerminalBackend>,
) -> Result<Option<bool>> {
    match res {
        ModalKeyResult::ThemeSelected { theme } => Ok(Some(handle_theme_selected(ctx, theme.clone()).await?)),
        ModalKeyResult::ThinkingLevelSelected { level, save_as_default } => Ok(Some(
            handle_thinking_selected(ctx, (level.clone(), *save_as_default)).await?,
        )),
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
    if let Some(handled) = handle_theme_or_thinking_selected(&res, ctx).await? {
        return Ok(handled);
    }
    match res {
        ModalKeyResult::LoginProviderSelected { provider } => handle_login_provider_selected(ctx, provider).await,
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

pub(crate) async fn apply_modal_key_result<B: TerminalBackend>(
    res: ModalKeyResult,
    mut ctx: ModalActionContext<'_, '_, '_, B>,
    batch: &mut LiveBatch,
) -> Result<bool> {
    dispatch_modal_result(res, &mut ctx, batch).await
}
