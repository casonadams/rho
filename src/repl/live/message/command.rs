use super::super::batch::drain_ui_events;
use super::super::turn::run_active_turn;
use super::super::{ActiveTurn, LiveMessage};
use super::session_cmd::{SessionCommandIo, handle_session_command};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::CommandResult;
use crate::ui::interactive::TerminalBackend;

pub(super) struct LiveCommandContext<'a, 'b> {
    pub session: &'a mut ReplSession,
    pub engine: &'b mut AgentEngine,
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

    if handle_session_command(
        &mut ctx,
        SessionCommandIo {
            controller: io.controller,
            history: editor.history,
            input: io.input,
        },
        result.clone(),
    )
    .await?
    {
        drain_ui_events(io.controller, io.events, &mut None)?;
        return Ok(false);
    }

    if super::auth_cmd::handle_auth_command(&mut ctx, &mut io, &result).await? {
        drain_ui_events(io.controller, io.events, &mut None)?;
        return Ok(false);
    }

    match result {
        CommandResult::Exit => return Ok(true),
        CommandResult::OpenModelSelector => {
            super::super::modal::open_model_selector(ctx.session, io.controller);
            io.controller.redraw()?;
        }
        CommandResult::OpenSettingsSelector => {
            super::super::modal::open_settings_selector(io.controller);
            io.controller.redraw()?;
        }
        CommandResult::OpenThemeSelector => {
            super::super::modal::open_theme_selector(ctx.session, io.controller);
            io.controller.redraw()?;
        }
        CommandResult::ThemeChanged { theme } => {
            let registry = crate::ui::theme::ThemeRegistry::new(Some(&ctx.session.config.config_dir));
            if let Some(resolved) = registry.get(&theme).cloned() {
                ctx.session.config.theme = theme.clone();
                ctx.session.renderer.theme = resolved.clone();
                let _ = io.controller.set_theme(resolved);
                let _ =
                    rho_harness_core::config::Config::set_file_value(&ctx.session.config.config_dir, "theme", &theme);
                ctx.session.renderer.print_status(&format!("Theme: {theme}"));
            }
        }
        CommandResult::ClearContext => {
            *ctx.engine =
                crate::platform::agent_engine(ctx.session.config.clone(), ctx.session.auth_store.clone(), None).await?;
        }
        CommandResult::ModelChanged {
            new_model,
            new_provider,
        } => {
            ctx.session.config.model = new_model.clone();
            if let Some(provider) = new_provider.as_ref() {
                ctx.session.config.provider = provider.clone();
            }
            let _ = rho_harness_core::state::AppState::set_last_model(
                &ctx.session.config.config_dir,
                &new_model,
                new_provider.as_deref(),
            );
            *ctx.engine = ctx
                .engine
                .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
                .await?;
        }
        CommandResult::Reload => {
            *ctx.engine = ctx.session.reload_engine(ctx.engine).await?;
        }
        CommandResult::Compact { instructions } => {
            ctx.session
                .renderer
                .print_notice("  [Compacting conversation context...]\n");
            match ctx.engine.compact_session(instructions.as_deref()).await {
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
        CommandResult::ExpandedPrompt { text } => {
            ctx.session.renderer.print_notice("  [Expanded template]\n");
            drain_ui_events(io.controller, io.events, &mut None)?;
            ctx.session.renderer.print_user_block(&text);
            run_active_turn(
                ctx.engine,
                &ctx.session.renderer,
                ActiveTurn {
                    io,
                    editor,
                    prompt: &text,
                },
            )
            .await?;
            ctx.engine.refresh_quota().await;
            return Ok(false);
        }
        CommandResult::Continue => {}
        _ => {}
    }
    drain_ui_events(io.controller, io.events, &mut None)?;
    Ok(false)
}
