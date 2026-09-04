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
    let LiveMessage { io, editor, message: _ } = live;

    if handle_session_command(
        &mut ctx,
        SessionCommandIo {
            controller: io.controller,
            history: editor.history,
        },
        result.clone(),
    )
    .await?
    {
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
        CommandResult::Login { provider } => {
            crate::cli::login_provider(provider.as_deref(), &ctx.session.config, &mut ctx.session.auth_store).await?;
            *ctx.engine = ctx
                .engine
                .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
                .await?;
        }
        CommandResult::Reload => {
            *ctx.engine = ctx.session.reload_engine(ctx.engine).await?;
        }
        CommandResult::Compact { .. } => {
            let session_id = ctx.engine.session_manager.session_id.clone();
            ctx.session
                .renderer
                .print_notice("  [Compacting conversation context...]\n");
            let memory = crate::session::context::context_memory(
                ctx.engine.session_manager.clone(),
                1,
                ctx.session.config.compaction_max_bytes,
            );
            let _ = memory.load(&session_id).await;
            ctx.session.renderer.print_notice("  [Context compaction completed]\n");
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
        CommandResult::Logout { provider } => {
            crate::cli::logout_provider(provider.as_deref(), &ctx.session.config, &mut ctx.session.auth_store)?;
            *ctx.engine = ctx
                .engine
                .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
                .await?;
        }
        CommandResult::Continue => {}
        _ => {}
    }
    drain_ui_events(io.controller, io.events, &mut None)?;
    Ok(false)
}
