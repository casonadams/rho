use super::super::batch::drain_ui_events;
use super::super::turn::run_active_turn;
use super::super::{ActiveTurn, EditorResources, LiveIo, LiveMessage};
use super::session_cmd::{SessionCommandIo, handle_session_command};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::CommandResult;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::TerminalBackend;

pub(super) struct LiveCommandContext<'a, 'b> {
    pub session: &'a mut ReplSession,
    pub engine: &'b mut AgentEngine,
}

async fn handle_theme_changed(
    ctx: &mut LiveCommandContext<'_, '_>,
    io_controller: &mut crate::ui::interactive::TerminalController<impl TerminalBackend>,
    theme: &str,
) {
    let registry = crate::ui::theme::ThemeRegistry::new(Some(&ctx.session.config.config_dir));
    if let Some(resolved) = registry.get(theme).cloned() {
        ctx.session.config.theme = theme.to_string();
        ctx.session.renderer.theme = resolved.clone();
        let _ = io_controller.set_theme(resolved);
        let _ = rho_harness_core::config::Config::set_file_value_async(&ctx.session.config.config_dir, "theme", theme)
            .await;
        ctx.session.renderer.print_status(&format!("Theme: {theme}"));
    }
}

async fn handle_selector_command(
    ctx: &mut LiveCommandContext<'_, '_>,
    io_controller: &mut crate::ui::interactive::TerminalController<impl TerminalBackend>,
    action: &CommandResult,
) -> Result<()> {
    match action {
        CommandResult::OpenModelSelector => super::super::modal::open_model_selector(ctx.session, io_controller),
        CommandResult::OpenSettingsSelector => super::super::modal::open_settings_selector(io_controller),
        CommandResult::OpenThemeSelector => super::super::modal::open_theme_selector(ctx.session, io_controller),
        CommandResult::OpenThinkingSelector => super::super::modal::open_thinking_selector(ctx.session, io_controller),
        CommandResult::OpenLoginSelector => super::super::modal::open_login_selector(ctx.session, io_controller),
        _ => {}
    }
    io_controller.redraw()?;
    Ok(())
}

async fn handle_thinking_changed(
    ctx: &mut LiveCommandContext<'_, '_>,
    io_controller: &mut crate::ui::interactive::TerminalController<impl TerminalBackend>,
    level: Option<&str>,
) {
    ctx.session.config.thinking_level = level.map(ToString::to_string);
    ctx.engine.config.thinking_level = ctx.session.config.thinking_level.clone();
    if let Err(err) = ctx.engine.update_model().await {
        ctx.session
            .renderer
            .print_notice(&format!("\nWarning: Could not update thinking level: {err}\n"));
    }
    super::super::navigation::update_footer(io_controller.state_mut(), ctx.session, ctx.engine);
    let _ = io_controller.redraw();
}

async fn handle_model_changed(ctx: &mut LiveCommandContext<'_, '_>, new_model: &str, new_provider: Option<&str>) {
    ctx.session.config.model = new_model.to_string();
    if let Some(provider) = new_provider {
        ctx.session.config.provider = provider.to_string();
    }
    let provider = ctx.session.config.provider.clone();
    if let Err(err) = ctx.engine.switch_model(new_model, &provider).await {
        ctx.session
            .renderer
            .print_notice(&format!("\nWarning: Could not switch model: {err}\n"));
    }
}

async fn handle_compact<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    instructions: Option<&str>,
) {
    ctx.session
        .renderer
        .print_notice("  [Compacting conversation context...]\n");
    match ctx.engine.compact_session(instructions).await {
        Ok(stats) => {
            let before = crate::ui::interactive::footer::format_tokens(stats.tokens_before as u64);
            let after = crate::ui::interactive::footer::format_tokens(stats.tokens_after as u64);
            let saved = crate::ui::interactive::footer::format_tokens(stats.saved_tokens as u64);
            ctx.session.renderer.print_notice(&format!(
                "  [Compacted context: {before} -> {after} tokens (saved {saved})]\n"
            ));
            super::super::turn::sync_turn_footer(io.controller, ctx.engine);
            let _ = io.controller.redraw();
        }
        Err(err) => {
            ctx.session
                .renderer
                .print_notice(&format!("  [Compaction failed: {err}]\n"));
        }
    }
}

async fn clear_engine_context(ctx: &mut LiveCommandContext<'_, '_>) -> Result<()> {
    *ctx.engine =
        crate::platform::agent_engine(ctx.session.config.clone(), ctx.session.auth_store.clone(), None).await?;
    Ok(())
}

async fn handle_model_or_thinking_command<B: TerminalBackend>(
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

async fn handle_engine_command_rest<B: TerminalBackend>(
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

async fn handle_engine_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut LiveIo<'_, B>,
    result: &CommandResult,
) -> Result<bool> {
    match result {
        CommandResult::OpenModelSelector
        | CommandResult::OpenSettingsSelector
        | CommandResult::OpenThemeSelector
        | CommandResult::OpenThinkingSelector
        | CommandResult::OpenLoginSelector => {
            handle_selector_command(ctx, io.controller, result).await?;
        }
        CommandResult::ThemeChanged { theme } => handle_theme_changed(ctx, io.controller, theme).await,
        CommandResult::ClearContext => clear_engine_context(ctx).await?,
        rest => return handle_engine_command_rest(ctx, io, rest).await,
    }
    Ok(true)
}

fn build_session_io<'a, 'b, B: TerminalBackend>(
    io: &'a mut LiveIo<'b, B>,
    history: &'a mut InteractiveHistory,
) -> SessionCommandIo<'a, B> {
    SessionCommandIo {
        controller: io.controller,
        history,
        input: io.input,
    }
}

fn flush_after_command<B: TerminalBackend>(io: &mut LiveIo<'_, B>) -> Result<()> {
    drain_ui_events(io.controller, io.events, &mut None)
}
pub(super) async fn handle_live_command<B: TerminalBackend>(
    mut ctx: LiveCommandContext<'_, '_>,
    live: LiveMessage<'_, B>,
    result: CommandResult,
) -> Result<bool> {
    let LiveMessage {
        mut io,
        editor,
        message: _,
    } = live;

    let session_io = build_session_io(&mut io, editor.history);
    if handle_session_command(&mut ctx, session_io, result.clone()).await? {
        flush_after_command(&mut io)?;
        return Ok(false);
    }
    if super::auth_cmd::handle_auth_command(&mut ctx, &mut io, &result).await? {
        flush_after_command(&mut io)?;
        return Ok(false);
    }
    if handle_engine_command(&mut ctx, &mut io, &result).await? {
        flush_after_command(&mut io)?;
        return Ok(false);
    }
    run_live_command_tail((ctx, io, editor, result)).await
}

async fn run_live_command_tail<B: TerminalBackend>(
    (ctx, mut io, editor, result): (
        LiveCommandContext<'_, '_>,
        LiveIo<'_, B>,
        EditorResources<'_>,
        CommandResult,
    ),
) -> Result<bool> {
    match result {
        CommandResult::Exit => Ok(true),
        CommandResult::ExpandedPrompt { text } => {
            ctx.session.renderer.print_notice("  [Expanded template]\n");
            flush_after_command(&mut io)?;
            ctx.session.renderer.print_user_block(&text);
            run_active_turn(
                ctx.session,
                ctx.engine,
                ActiveTurn {
                    io,
                    editor,
                    prompt: &text,
                },
            )
            .await?;
            ctx.session.sync_engine_model(ctx.engine).await;
            ctx.engine.refresh_quota().await;
            Ok(false)
        }
        _ => flush_after_command(&mut io).map(|_| false),
    }
}
