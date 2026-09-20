//! Live message processing, bash escape expansion, and slash command dispatch.

mod commands;
mod prompt;
mod session_ops;
pub(crate) mod types;

pub(super) use prompt::resolve_effective_prompt;
pub(super) use types::LiveCommandContext;

use commands::{handle_auth_command, handle_engine_command};
use prompt::{is_slash_input, run_prompt_turn, run_slash_handler};
use session_ops::handle_session_command;
use types::SessionCommandIo;

use super::batch::drain_ui_events;
use super::turn::run_active_turn;
use super::{ActiveTurn, EditorResources, LiveIo, LiveMessage};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::CommandResult;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{Activity, TerminalBackend};

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
    if handle_auth_command(&mut ctx, &mut io, &result).await? {
        flush_after_command(&mut io)?;
        return Ok(false);
    }
    if handle_engine_command(&mut ctx, &mut io, &result).await? {
        flush_after_command(&mut io)?;
        return Ok(false);
    }
    run_live_command_tail(ctx, io, editor, result).await
}

async fn run_live_command_tail<B: TerminalBackend>(
    ctx: LiveCommandContext<'_, '_>,
    mut io: LiveIo<'_, B>,
    editor: EditorResources<'_>,
    result: CommandResult,
) -> Result<bool> {
    match result {
        CommandResult::Exit => Ok(true),
        CommandResult::ExpandedPrompt { text } => {
            ctx.session.renderer.print_notice("  [Expanded template]\n");
            flush_after_command(&mut io)?;
            io.controller.state_mut().footer_mut().activity = Activity::Working;
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
            ctx.engine.spawn_refresh_quota();
            Ok(false)
        }
        _ => flush_after_command(&mut io).map(|_| false),
    }
}

impl ReplSession {
    pub(super) async fn process_live_message<B: TerminalBackend>(
        &mut self,
        engine: &mut AgentEngine,
        mut live: LiveMessage<'_, B>,
    ) -> Result<bool> {
        let input = live.message.text.clone();
        if is_slash_input(&input) {
            let Some(result) = run_slash_handler(self, engine, &input).await? else {
                return Ok(false);
            };
            return handle_live_command(LiveCommandContext { session: self, engine }, live, result).await;
        }

        let Some(effective) = resolve_effective_prompt(&input, &self.renderer, &mut live.io).await? else {
            return Ok(false);
        };
        run_prompt_turn(self, engine, live, effective).await
    }
}
