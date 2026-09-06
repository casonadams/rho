mod auth_cmd;
mod bash_escape;
mod branch;
mod command;
mod session_cmd;

use super::batch::drain_ui_events;
use super::turn::run_active_turn;
use super::{ActiveTurn, LiveMessage};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};

use bash_escape::resolve_effective_prompt;
use command::{LiveCommandContext, handle_live_command};

fn is_slash_input(input: &str) -> bool {
    crate::repl::commands::is_slash_command(input)
}

async fn run_slash_handler(
    session: &mut ReplSession,
    engine: &mut AgentEngine,
    input: &str,
) -> Result<Option<CommandResult>> {
    let mut command_context = SlashCommandContext {
        config: &mut session.config,
        auth_store: &mut session.auth_store,
        renderer: &session.renderer,
        session_id: Some(&engine.session_manager.session_id),
        session_manager: Some(&engine.session_manager),
        engine: Some(engine),
        home_dir: None,
    };
    SlashCommandHandler::handle(input, &mut command_context).await
}

async fn run_prompt_turn<B: crate::ui::interactive::TerminalBackend>(
    (session, engine): (&mut ReplSession, &mut AgentEngine),
    (live, effective): (LiveMessage<'_, B>, String),
) -> Result<bool> {
    session.renderer.print_user_block(&effective);
    run_active_turn(
        session,
        engine,
        ActiveTurn {
            io: live.io,
            editor: live.editor,
            prompt: &effective,
        },
    )
    .await?;
    session.sync_engine_model(engine).await;
    engine.refresh_quota().await;
    Ok(false)
}

impl ReplSession {
    pub(super) async fn process_live_message<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        engine: &mut AgentEngine,
        mut live: LiveMessage<'_, B>,
    ) -> Result<bool> {
        let input = live.message.text.trim().to_string();
        if is_slash_input(&input)
            && let Some(result) = run_slash_handler(self, engine, &input).await?
        {
            return handle_live_command(LiveCommandContext { session: self, engine }, live, result).await;
        }

        let Some(effective) = resolve_effective_prompt(&input, &self.renderer, &mut live.io).await? else {
            drain_ui_events(live.io.controller, live.io.events, &mut None)?;
            return Ok(false);
        };

        run_prompt_turn((self, engine), (live, effective)).await
    }
}
