use super::types::{LiveCommandContext, SessionCommandIo};
use crate::error::{AppError, Result};
use crate::repl::commands::CommandResult;
use crate::repl::live::LiveIo;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub(crate) fn open_selector_modal<B: TerminalBackend>(
    session: &crate::repl::ReplSession,
    io_controller: &mut TerminalController<B>,
    action: &CommandResult,
) {
    match action {
        CommandResult::OpenModelSelector => crate::repl::live::modal::open_model_selector(session, io_controller),
        CommandResult::OpenSettingsSelector => crate::repl::live::modal::open_settings_selector(
            Some(&session.config.model),
            session.config.guard_model(),
            session.config.thinking_level.as_deref(),
            io_controller,
        ),
        CommandResult::OpenHelpSelector => crate::repl::live::modal::open_help_selector(io_controller),
        CommandResult::OpenLoginSelector => crate::repl::live::modal::open_login_selector(session, io_controller),
        CommandResult::OpenMcpSelector => crate::repl::live::modal::open_mcp_selector(session, io_controller),
        _ => {}
    }
}

pub(crate) async fn handle_selector_command(
    ctx: &mut LiveCommandContext<'_, '_>,
    io_controller: &mut TerminalController<impl TerminalBackend>,
    action: &CommandResult,
) -> Result<()> {
    open_selector_modal(ctx.session, io_controller, action);
    io_controller.redraw()?;
    Ok(())
}

pub(crate) async fn handle_thinking_changed(
    ctx: &mut LiveCommandContext<'_, '_>,
    io_controller: &mut TerminalController<impl TerminalBackend>,
    level: Option<&str>,
) {
    ctx.session.config.thinking_level = level.map(ToString::to_string);
    ctx.engine.config.thinking_level = ctx.session.config.thinking_level.clone();
    if let Err(err) = ctx.engine.update_model().await {
        ctx.session.renderer.notice(
            crate::ui::render::NoticeKind::Warn,
            &format!("Could not update thinking level: {err}"),
        );
    }
    crate::repl::live::navigation::update_footer(io_controller.state_mut(), ctx.session, ctx.engine);
    let _ = io_controller.redraw();
}

pub(crate) async fn handle_model_changed(
    ctx: &mut LiveCommandContext<'_, '_>,
    new_model: &str,
    new_provider: Option<&str>,
) {
    ctx.session.config.model = new_model.to_string();
    if let Some(provider) = new_provider {
        ctx.session.config.provider = provider.to_string();
    }
    let provider = ctx.session.config.provider.clone();
    if let Err(err) = ctx.engine.switch_model(new_model, &provider).await {
        ctx.session.renderer.notice(
            crate::ui::render::NoticeKind::Warn,
            &format!("Could not switch model: {err}"),
        );
    }
}

pub(crate) async fn handle_compact<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    instructions: Option<&str>,
) {
    ctx.session
        .renderer
        .print_notice("  [Compacting conversation context...]\n");
    match ctx.engine.compact_session(instructions).await {
        Ok(stats) => {
            crate::repl::line_mode::dispatch::print_compaction_stats(ctx.session, &stats);
            crate::repl::live::turn::sync_turn_footer(io.controller, ctx.engine);
            let _ = io.controller.redraw();
        }
        Err(err) => {
            ctx.session
                .renderer
                .print_notice(&format!("  [Compaction failed: {err}]\n"));
        }
    }
}

pub(crate) async fn clear_engine_context(ctx: &mut LiveCommandContext<'_, '_>) -> Result<()> {
    *ctx.engine =
        crate::platform::agent_engine(ctx.session.config.clone(), ctx.session.auth_store.clone(), None).await?;
    Ok(())
}

pub(crate) async fn handle_model_or_thinking_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    result: &CommandResult,
) -> bool {
    match result {
        CommandResult::ModelChanged {
            new_model,
            new_provider,
        } => {
            handle_model_changed(ctx, new_model, new_provider.as_deref()).await;
            true
        }
        CommandResult::ThinkingChanged { level } => {
            handle_thinking_changed(ctx, io.controller, level.as_deref()).await;
            true
        }
        _ => false,
    }
}

pub(crate) async fn handle_engine_command_rest<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    result: &CommandResult,
) -> Result<bool> {
    if handle_model_or_thinking_command(ctx, io, result).await {
        return Ok(true);
    }
    match result {
        CommandResult::Reload => {
            *ctx.engine = ctx.session.reload_engine(ctx.engine).await?;
            Ok(true)
        }
        CommandResult::Compact { instructions } => {
            handle_compact(ctx, io, instructions.as_deref()).await;
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub(crate) async fn handle_engine_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    result: &CommandResult,
) -> Result<bool> {
    match result {
        CommandResult::OpenModelSelector
        | CommandResult::OpenSettingsSelector
        | CommandResult::OpenHelpSelector
        | CommandResult::OpenLoginSelector
        | CommandResult::OpenMcpSelector => {
            handle_selector_command(ctx, io.controller, result).await?;
        }
        CommandResult::ClearContext => clear_engine_context(ctx).await?,
        rest => return handle_engine_command_rest(ctx, io, rest).await,
    }
    Ok(true)
}

pub(crate) fn handle_auth_result(
    ctx: &mut LiveCommandContext<'_, '_>,
    res: std::result::Result<(), AppError>,
    verb: &str,
) {
    match res {
        Ok(()) => {}
        Err(AppError::Cancelled(_)) => {}
        Err(err) => ctx.session.renderer.print_notice(&format!("  {verb} failed: {err}\n")),
    }
}

pub(crate) async fn rebuild_after_auth(ctx: &mut LiveCommandContext<'_, '_>) -> Result<()> {
    *ctx.engine = ctx
        .engine
        .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
        .await?;
    crate::repl::report_mcp_diagnostics(ctx.session, ctx.engine);
    Ok(())
}

async fn execute_live_login<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    provider: Option<&str>,
) -> Result<()> {
    if let Some(server) = provider.and_then(|p| p.strip_prefix("mcp:")) {
        let theme = ctx.session.renderer.theme.clone();
        let mut ui = crate::repl::live::mcp_login::McpLoginUi {
            controller: io.controller,
            input: io.input,
            theme: &theme,
            open_url: crate::repl::live::mcp_login::system_browser(),
        };
        if crate::repl::live::mcp_login::run_mcp_login(&mut ui, server, &ctx.session.config)
            .await
            .is_ok()
        {
            rebuild_after_auth(ctx).await?;
        }
        return Ok(());
    }
    let res = io
        .suspend_for_async(|| {
            crate::cli::login_provider(provider, false, &ctx.session.config, &mut ctx.session.auth_store)
        })
        .await?;
    handle_auth_result(ctx, res, "Login");
    rebuild_after_auth(ctx).await
}

fn execute_live_logout<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    provider: Option<&str>,
) -> Result<()> {
    let res =
        io.suspend_for(|| crate::cli::logout_provider(provider, &ctx.session.config, &mut ctx.session.auth_store))?;
    handle_auth_result(ctx, res, "Logout");
    Ok(())
}

pub(crate) async fn handle_auth_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    result: &CommandResult,
) -> Result<bool> {
    match result {
        CommandResult::Login { provider } => {
            execute_live_login(ctx, io, provider.as_deref()).await?;
            Ok(true)
        }
        CommandResult::Logout { provider } => {
            execute_live_logout(ctx, io, provider.as_deref())?;
            rebuild_after_auth(ctx).await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn print_tree_view(
    session: &crate::repl::ReplSession,
    session_id: &str,
    tree: &rho_harness_core::session::tree::SessionTree,
) {
    let rendered = crate::ui::interactive::tree_view::render_tree_ascii(tree);
    session
        .renderer
        .print_notice(&format!("\nConversation Tree (Session: {session_id}):\n{rendered}\n"));
}

pub(crate) async fn handle_tree_commands<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<()> {
    let tree = ctx.engine.session_manager.load_tree().await?;
    if *result == CommandResult::OpenTreeSelector {
        crate::repl::live::modal::open_tree_selector(&tree, io.controller);
        io.controller.redraw()?;
    } else if *result == CommandResult::Tree {
        print_tree_view(ctx.session, &ctx.engine.session_manager.session_id, &tree);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::interactive::NullBackend;
    use crate::ui::interactive::{InteractiveState, TerminalController};

    #[test]
    fn test_open_selector_modal_actions() {
        let config = rho_harness_core::config::Config::default();
        let auth = crate::auth::AuthStore::default();
        let session = crate::repl::ReplSession::new(config, auth, None);

        let mut controller = TerminalController::new(NullBackend, InteractiveState::default()).unwrap();
        open_selector_modal(&session, &mut controller, &CommandResult::OpenModelSelector);
        assert!(controller.state().active_modal().is_some());
        controller.state_mut().pop_modal();

        open_selector_modal(&session, &mut controller, &CommandResult::OpenSettingsSelector);
        assert!(controller.state().active_modal().is_some());
        controller.state_mut().pop_modal();

        open_selector_modal(&session, &mut controller, &CommandResult::OpenHelpSelector);
        assert!(controller.state().active_modal().is_some());
        controller.state_mut().pop_modal();

        open_selector_modal(&session, &mut controller, &CommandResult::OpenLoginSelector);
        assert!(controller.state().active_modal().is_some());
        controller.state_mut().pop_modal();

        open_selector_modal(&session, &mut controller, &CommandResult::OpenMcpSelector);
        assert!(controller.state().active_modal().is_some());
        controller.state_mut().pop_modal();

        open_selector_modal(&session, &mut controller, &CommandResult::Continue);
        assert!(controller.state().active_modal().is_none());
    }
}
