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

use crate::engine::AgentEngine;
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

async fn apply_cli_session_name(engine: &AgentEngine, cli: Option<&crate::config::cli::Cli>) {
    if let Some(name) = cli.and_then(|c| c.name.as_deref()) {
        let _ = engine.session_manager.set_session_name(name).await;
    }
}

fn apply_initial_theme(session: &mut ReplSession) {
    let registry = crate::ui::theme::ThemeRegistry::new(Some(&session.config.config_dir));
    if let Some(initial_theme) = registry.get(&session.config.theme).cloned() {
        session.renderer.theme = initial_theme;
    }
}

async fn init_line_mode(session: &mut ReplSession) -> Result<AgentEngine> {
    let engine = crate::platform::agent_engine(
        session.config.clone(),
        session.auth_store.clone(),
        session.resume_id.as_deref(),
    )
    .await?;
    apply_cli_session_name(&engine, session.cli.as_ref()).await;
    session.config = engine.config.clone();
    engine.refresh_quota().await;
    apply_initial_theme(session);
    print_line_mode_welcome(session, &engine).await;
    Ok(engine)
}

async fn read_next_line(mut editor: reedline::Reedline) -> Result<(reedline::Reedline, std::io::Result<Signal>)> {
    tokio::task::spawn_blocking(move || {
        let sig = editor.read_line(&SimplePrompt);
        (editor, sig)
    })
    .await
    .map_err(|e| anyhow::anyhow!("Line editor task failed: {e}").into())
}

async fn handle_line_slash_command(
    input: &str,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<Option<DispatchOutcome>> {
    if !crate::repl::commands::is_slash_command(input) {
        return Ok(None);
    }
    let mut ctx = SlashCommandContext {
        config: &mut session.config,
        auth_store: &mut session.auth_store,
        renderer: &session.renderer,
        session_id: Some(&engine.session_manager.session_id),
        session_manager: Some(&engine.session_manager),
        engine: Some(engine),
        home_dir: None,
    };
    let Some(cmd_res) = SlashCommandHandler::handle(input, &mut ctx).await? else {
        return Ok(None);
    };
    handle_command_result(cmd_res, session, engine).await.map(Some)
}

async fn execute_user_input(
    input: &str,
    (session, engine): (&ReplSession, &mut AgentEngine),
    stdin_is_tty: bool,
) -> Result<()> {
    let effective = match handle_shell_command(input, &session.renderer).await {
        ShellAction::Handled => return Ok(()),
        ShellAction::Prompt(p) => p,
        ShellAction::Passthrough => input.to_string(),
    };
    if stdin_is_tty {
        clear_submitted_input(input);
    }
    session.renderer.print_user_block(&effective);
    session.renderer.write_output("\n");
    run_agent_turn(
        engine,
        &session.renderer,
        crate::engine::runner::TurnRequest::new(&effective),
    )
    .await?;
    engine.refresh_quota().await;
    Ok(())
}

async fn run_dispatch_turn(text: &str, session: &mut ReplSession, engine: &mut AgentEngine) -> Result<bool> {
    run_agent_turn(engine, &session.renderer, crate::engine::runner::TurnRequest::new(text)).await?;
    engine.refresh_quota().await;
    Ok(true)
}

async fn process_line_input(
    buffer: &str,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
    stdin_is_tty: bool,
) -> Result<bool> {
    let input = buffer.trim();
    if input.is_empty() {
        return Ok(true);
    }
    if let Some(outcome) = handle_line_slash_command(input, session, engine).await? {
        return match outcome {
            DispatchOutcome::Continue => Ok(true),
            DispatchOutcome::Break => Ok(false),
            DispatchOutcome::RunTurn(text) => run_dispatch_turn(&text, session, engine).await,
        };
    }
    execute_user_input(input, (session, engine), stdin_is_tty).await?;
    Ok(true)
}

async fn handle_line_signal(
    sig: std::io::Result<Signal>,
    (session, engine): (&mut ReplSession, &mut AgentEngine),
    stdin_is_tty: bool,
) -> Result<bool> {
    match sig {
        Ok(Signal::Success(buffer)) => process_line_input(&buffer, session, engine, stdin_is_tty).await,
        Ok(Signal::CtrlC) => {
            session.renderer.write_output("\nCanceled input.\n");
            Ok(true)
        }
        Ok(Signal::CtrlD) => {
            session.renderer.write_output("\nBye.\n");
            Ok(false)
        }
        Err(err) => {
            session.renderer.write_output(&format!("Input error: {err}\n"));
            Ok(false)
        }
    }
}

fn render_line_mode_prompt(session: &ReplSession, engine: &AgentEngine) {
    let quota = engine.quota_display();
    session.renderer.print_session_status(&SessionStatus {
        model: session.config.model.clone(),
        provider: session.config.provider.clone(),
        context: engine.context_remaining_display(),
        quota,
    });
}

async fn run_line_mode_loop(
    (session, engine): (&mut ReplSession, &mut AgentEngine),
    line_editor: reedline::Reedline,
    stdin_is_tty: bool,
) -> Result<()> {
    let mut line_editor = line_editor;
    let mut is_first_prompt = true;
    loop {
        if is_first_prompt {
            is_first_prompt = false;
        } else {
            session.renderer.write_output("\n");
        }
        render_line_mode_prompt(session, engine);

        let (next_editor, sig) = read_next_line(line_editor).await?;
        line_editor = next_editor;
        if !handle_line_signal(sig, (session, engine), stdin_is_tty).await? {
            break;
        }
    }
    Ok(())
}

pub async fn run_line_mode(session: &mut ReplSession, stdin_is_tty: bool) -> Result<()> {
    let mut engine = init_line_mode(session).await?;
    let line_editor = build_line_editor(&session.config, &session.auth_store)?;
    run_line_mode_loop((session, &mut engine), line_editor, stdin_is_tty).await
}
