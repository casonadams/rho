pub mod dispatch;
pub mod editor;
pub mod shell;
#[cfg(test)]
mod tests;
pub mod tree;
pub mod turn;

pub use shell::clear_submitted_input;
#[cfg(test)]
pub use shell::submitted_input_rows;

use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::{SlashCommandContext, SlashCommandHandler};
use crate::repl::prompt::SimplePrompt;
use crate::ui::render::SessionStatus;
use dispatch::{DispatchOutcome, handle_command_result};
use editor::{build_line_editor, print_line_mode_welcome};
use reedline::Signal;
use shell::{ShellAction, handle_shell_command};
use turn::run_agent_turn;

pub async fn run_line_mode(session: &mut ReplSession, stdin_is_tty: bool) -> Result<()> {
    let mut engine = crate::platform::agent_engine(
        session.config.clone(),
        session.auth_store.clone(),
        session.resume_id.as_deref(),
    )
    .await?;
    if let Some(ref cli) = session.cli
        && let Some(ref name) = cli.name
    {
        let _ = engine.session_manager.set_session_name(name).await;
    }
    session.config = engine.config.clone();
    engine.refresh_quota().await;

    print_line_mode_welcome(session, &engine);

    let mut line_editor = build_line_editor(&session.config, &session.auth_store)?;
    let prompt = SimplePrompt;
    let mut is_first_prompt = true;

    loop {
        if !is_first_prompt {
            session.renderer.write_output("\n");
        }
        is_first_prompt = false;

        let quota = engine.quota_display();
        session.renderer.print_session_status(&SessionStatus {
            model: session.config.model.clone(),
            provider: session.config.provider.clone(),
            context: engine.context_remaining_display(),
            quota,
            auto_approve: session.config.auto_approve,
        });

        let sig = line_editor.read_line(&prompt);
        match sig {
            Ok(Signal::Success(buffer)) => {
                let input = buffer.trim();
                if input.is_empty() {
                    continue;
                }

                if input.starts_with('/') {
                    let mut command_context = SlashCommandContext {
                        config: &mut session.config,
                        auth_store: &mut session.auth_store,
                        renderer: &session.renderer,
                        session_id: Some(&engine.session_manager.session_id),
                        session_manager: Some(&engine.session_manager),
                        engine: Some(&engine),
                        home_dir: None,
                    };
                    let result = SlashCommandHandler::handle(input, &mut command_context).await?;
                    if let Some(cmd_res) = result {
                        match handle_command_result(cmd_res, session, &mut engine).await? {
                            DispatchOutcome::Continue => continue,
                            DispatchOutcome::Break => break,
                            DispatchOutcome::RunTurn(text) => {
                                run_agent_turn(
                                    &engine,
                                    &session.renderer,
                                    crate::engine::runner::TurnRequest::new(&text),
                                )
                                .await?;
                                engine.refresh_quota().await;
                                continue;
                            }
                        }
                    }
                }

                let effective_input = match handle_shell_command(input, &session.renderer).await {
                    ShellAction::Handled => continue,
                    ShellAction::Prompt(p) => p,
                    ShellAction::Passthrough => input.to_string(),
                };

                if stdin_is_tty {
                    clear_submitted_input(input);
                }
                session.renderer.print_user_block(&effective_input);
                session.renderer.write_output("\n");
                run_agent_turn(
                    &engine,
                    &session.renderer,
                    crate::engine::runner::TurnRequest::new(&effective_input),
                )
                .await?;
                engine.refresh_quota().await;
            }
            Ok(Signal::CtrlC) => {
                session.renderer.write_output("\nCanceled input.\n");
            }
            Ok(Signal::CtrlD) => {
                session.renderer.write_output("\nBye.\n");
                break;
            }
            Err(err) => {
                session.renderer.write_output(&format!("Input error: {err}\n"));
                break;
            }
        }
    }

    Ok(())
}
