use super::super::batch::LiveBatch;
use super::super::modal::ModalKeyResult;
use super::super::modal::actions::{
    apply_shared_modal_action, open_shared_help_modal, report_save, save_or_print_thinking,
};
use super::super::navigation::{hydrate_session_transcript, update_footer};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::input_reader::TerminalInputReader;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub(crate) struct ModalActionContext<'a, B: TerminalBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub session: &'a mut ReplSession,
    pub engine: &'a mut AgentEngine,
    pub input: &'a mut TerminalInputReader,
}

fn print_model_status(ctx: &mut ModalActionContext<'_, impl TerminalBackend>, saved_default: bool) {
    let label = if saved_default { "Default model" } else { "Model" };
    let spec = ctx.session.config.canonical_model_spec();
    ctx.session.renderer.print_status(&format!("{label}: {spec}"));
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
    let saved_default = save_as_default && report_save(&ctx.session.renderer, "default model", &saved);
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
                let _ = hydrate_session_transcript(ctx.controller, &tree);
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
        let _ = hydrate_session_transcript(ctx.controller, &tree);
        if !tree.is_empty() && ctx.session.config.recap.on_resume {
            ctx.session.pending_resume_recap = true;
        }
    }
    ctx.session
        .renderer
        .print_status(&format!("Resumed session {session_id}"));
    update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
    ctx.controller.redraw()?;
    Ok(true)
}

async fn handle_thinking_selected(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    level: Option<String>,
    save_as_default: bool,
) -> Result<bool> {
    let changed = ctx.engine.config.thinking_level != level;
    ctx.session.config.thinking_level = level.clone();
    ctx.engine.config.thinking_level = level.clone();
    save_or_print_thinking(ctx.session, level.as_deref(), save_as_default).await;
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

async fn open_idle_help_modal(
    command: &str,
    session: &ReplSession,
    engine: &AgentEngine,
    controller: &mut TerminalController<impl TerminalBackend>,
) -> Result<bool> {
    if open_shared_help_modal(command, session, engine, controller)? {
        return Ok(true);
    }
    match command {
        "/resume" => {
            super::super::modal::open_session_selector(
                &session.config.sessions_dir,
                &engine.session_manager.session_id,
                controller,
            )
            .await;
        }
        "/mcp" => super::super::modal::open_mcp_selector(session, controller),
        "/login" => super::super::modal::open_login_selector(session, controller),
        "/tree" => {
            if let Ok(tree) = engine.session_manager.load_tree().await {
                super::super::modal::open_tree_selector(&tree, controller);
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

async fn handle_help_command_selected(
    ctx: &mut ModalActionContext<'_, impl TerminalBackend>,
    command: &str,
) -> Result<bool> {
    open_idle_help_modal(command, ctx.session, ctx.engine, ctx.controller).await?;
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
        action => {
            if crate::repl::live::modal::btw::handle_btw_modal_action(ctx.session, ctx.engine, ctx.controller, &action)
                .await
            {
                batch.flush(ctx.controller, true)?;
                return Ok(true);
            }
            if apply_shared_modal_action(ctx.session, ctx.engine, ctx.controller, &action).await? {
                ctx.session.sync_engine_settings(ctx.engine);
                return Ok(true);
            }
            if handle_session_modal_action(ctx, action.clone()).await? {
                return Ok(true);
            }
            handle_selection_action(ctx, action, batch).await
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
