pub mod dispatch;
pub mod shell;
#[cfg(test)]
mod tests;

pub use shell::clear_submitted_input;
#[cfg(test)]
pub use shell::submitted_input_rows;

use crate::auth::AuthStore;
use crate::config::Config;
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::{SlashCommandContext, SlashCommandHandler};
use crate::repl::completer::RhoCompleter;
use crate::repl::prompt::SimplePrompt;
use crate::ui::TerminalRenderer;
use crate::ui::render::{SessionStatus, WelcomeDisplay};
use dispatch::{DispatchOutcome, handle_command_result};
use reedline::{
    ColumnarMenu, Emacs, FileBackedHistory, KeyCode, KeyModifiers, MenuBuilder, Reedline, ReedlineEvent, ReedlineMenu,
    Signal, default_emacs_keybindings,
};
use shell::{ShellAction, handle_shell_command};

async fn build_completion_sources_async(
    config: &Config,
    auth_store: &AuthStore,
) -> crate::repl::interactive::CompletionSources {
    let cwd = std::env::current_dir().ok();
    let skills = crate::skills::resolved_skills_async(cwd.as_deref()).await;
    let prompt_templates =
        rho_harness_core::prompts::discover_prompt_templates_async(Some(&config.config_dir), cwd.as_deref())
            .await
            .into_iter()
            .map(|t| t.metadata.name)
            .collect();
    let models = crate::repl::interactive::discover_models(config, auth_store);
    let custom_providers = config.providers.keys().cloned().collect();
    crate::repl::interactive::CompletionSources::new()
        .with_skills(skills)
        .with_templates(prompt_templates)
        .with_models(models)
        .with_custom_providers(custom_providers)
}

fn build_emacs_edit_mode() -> Box<Emacs> {
    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::ALT,
        KeyCode::Enter,
        ReedlineEvent::Edit(vec![reedline::EditCommand::InsertNewline]),
    );
    Box::new(Emacs::new(keybindings))
}

pub async fn build_line_editor_async(config: &Config, auth_store: &AuthStore) -> Result<Reedline> {
    let edit_mode = build_emacs_edit_mode();
    let sources = build_completion_sources_async(config, auth_store).await;
    let completer = Box::new(RhoCompleter::new(sources));
    let completion_menu = Box::new(ColumnarMenu::default().with_name("completion_menu"));

    let history = Box::new(
        FileBackedHistory::with_file(1000, config.config_dir.join("history.txt"))
            .map_err(|error| anyhow::anyhow!("History unavailable: {error}"))?,
    );

    Ok(Reedline::create()
        .with_history(history)
        .with_completer(completer)
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode))
}

pub async fn print_line_mode_welcome(session: &ReplSession, engine: &AgentEngine) {
    let skills = crate::skills::resolved_skills_async(std::env::current_dir().ok().as_deref()).await;
    let skill_names: Vec<String> = skills.iter().map(|s| s.metadata.name.clone()).collect();
    let tools = engine.tool_names();
    let mcp = session.config.mcp.servers.keys().cloned().collect::<Vec<_>>();
    let agents = engine.instruction_files().await;

    session.renderer.print_welcome(&WelcomeDisplay {
        model: session.config.model.clone(),
        provider: session.config.provider.clone(),
        resumed: session.resume_id.is_some(),
        agents,
        tools,
        skills: skill_names,
        mcp,
    });
}

async fn handle_turn_cancellation(engine: &AgentEngine, renderer: &TerminalRenderer) -> Result<()> {
    rho_engine::process::kill_all_tracked_processes();
    renderer.flush();
    engine.record_cancellation("operator interrupt").await?;
    renderer.write_output("\nCanceled.\n");
    Ok(())
}

pub(crate) fn handle_turn_completion(res: Result<crate::engine::runner::TurnOutput>, renderer: &TerminalRenderer) {
    renderer.flush();
    renderer.write_output("\n");
    match res {
        Ok(out) if out.status == crate::engine::runner::RunStatus::Compacted => {
            renderer.write_output("Context was compacted. Submit your prompt to proceed with compacted context.\n");
        }
        Err(error) => {
            renderer.write_output(&format!("\nError: {error}\n"));
        }
        _ => {}
    }
}

enum TurnSignal {
    Done(Box<Result<crate::engine::runner::TurnOutput>>),
    Interrupted,
}

async fn wait_turn_or_interrupt(
    future: impl std::future::Future<Output = Result<crate::engine::runner::TurnOutput>>,
) -> TurnSignal {
    tokio::pin!(future);
    tokio::select! {
        res = &mut future => TurnSignal::Done(Box::new(res)),
        _ = tokio::signal::ctrl_c() => TurnSignal::Interrupted,
    }
}

pub async fn run_agent_turn(
    engine: &AgentEngine,
    renderer: &TerminalRenderer,
    request: crate::engine::runner::TurnRequest<'_>,
) -> Result<()> {
    match wait_turn_or_interrupt(engine.run_turn(request, std::sync::Arc::new(renderer.clone()))).await {
        TurnSignal::Done(res) => {
            handle_turn_completion(*res, renderer);
            Ok(())
        }
        TurnSignal::Interrupted => handle_turn_cancellation(engine, renderer).await,
    }
}

async fn apply_cli_session_name(engine: &AgentEngine, cli: Option<&crate::config::cli::Cli>) {
    if let Some(name) = cli.and_then(|c| c.name.as_deref()) {
        let _ = engine.session_manager.set_session_name(name).await;
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
    engine.spawn_refresh_quota();
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
    session: &ReplSession,
    engine: &mut AgentEngine,
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
    engine.spawn_refresh_quota();
    Ok(())
}

async fn run_dispatch_turn(text: &str, session: &mut ReplSession, engine: &mut AgentEngine) -> Result<bool> {
    run_agent_turn(engine, &session.renderer, crate::engine::runner::TurnRequest::new(text)).await?;
    engine.spawn_refresh_quota();
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
    execute_user_input(input, session, engine, stdin_is_tty).await?;
    Ok(true)
}

fn render_line_mode_prompt(session: &ReplSession, engine: &AgentEngine) {
    let quota = engine.quota_display();
    let context = match engine.cache_hit_display() {
        Some(hit) => format!("{} ({hit})", engine.context_remaining_display()),
        None => engine.context_remaining_display(),
    };
    session.renderer.print_session_status(&SessionStatus {
        model: session.config.model.clone(),
        provider: session.config.provider.clone(),
        context,
        quota,
    });
}

async fn handle_line_signal(
    sig: std::io::Result<Signal>,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
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
        Ok(_) => Ok(true),
        Err(err) => {
            session.renderer.write_output(&format!("Input error: {err}\n"));
            Ok(false)
        }
    }
}

pub async fn run_line_mode(session: &mut ReplSession, stdin_is_tty: bool) -> Result<()> {
    let mut engine = init_line_mode(session).await?;
    let mut line_editor = build_line_editor_async(&session.config, &session.auth_store).await?;
    let mut is_first_prompt = true;

    loop {
        if is_first_prompt {
            is_first_prompt = false;
        } else {
            session.renderer.write_output("\n");
        }
        render_line_mode_prompt(session, &engine);

        let (next_editor, sig) = read_next_line(line_editor).await?;
        line_editor = next_editor;

        if !handle_line_signal(sig, session, &mut engine, stdin_is_tty).await? {
            break;
        }
    }

    Ok(())
}
