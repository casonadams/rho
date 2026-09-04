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
use crate::repl::commands::{SlashCommandContext, SlashCommandHandler};

use bash_escape::resolve_effective_prompt;
use command::{LiveCommandContext, handle_live_command};

impl ReplSession {
    pub(super) async fn process_live_message<B: crate::ui::interactive::TerminalBackend>(
        &mut self,
        engine: &mut AgentEngine,
        mut live: LiveMessage<'_, B>,
    ) -> Result<bool> {
        let input = live.message.text.trim().to_string();
        if input.starts_with('/') {
            let command_result = {
                let mut command_context = SlashCommandContext {
                    config: &mut self.config,
                    auth_store: &mut self.auth_store,
                    renderer: &self.renderer,
                    session_id: Some(&engine.session_manager.session_id),
                    session_manager: Some(&engine.session_manager),
                    engine: Some(engine),
                    home_dir: None,
                };
                SlashCommandHandler::handle(&input, &mut command_context).await?
            };
            if let Some(result) = command_result {
                return handle_live_command(LiveCommandContext { session: self, engine }, live, result).await;
            }
        }

        let Some(effective) = resolve_effective_prompt(&input, &self.renderer, &mut live.io).await? else {
            drain_ui_events(live.io.controller, live.io.events, &mut None)?;
            return Ok(false);
        };

        self.renderer.print_user_block(&effective);
        run_active_turn(
            engine,
            &self.renderer,
            ActiveTurn {
                io: live.io,
                editor: live.editor,
                prompt: &effective,
            },
        )
        .await?;
        engine.refresh_quota().await;
        Ok(false)
    }
}
