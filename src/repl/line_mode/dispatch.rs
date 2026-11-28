use std::io::{BufRead, IsTerminal, Write};

use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::branch_summary::{SUMMARY_PROMPT, has_assistant_turn, should_summarize_branch, summarize_abandoned};
use crate::repl::commands::CommandResult;
use crate::repl::commands::types::WorkflowLaunch;
use crate::repl::model_switch::{print_session_model, switch_session_model};
use crate::repl::session_ops::{self, RecapOutcome};

pub enum DispatchOutcome {
    Continue,
    Break,
    RunTurn(String),
}

pub async fn show_tree(session: &ReplSession, engine: &AgentEngine) -> Result<()> {
    let tree = engine.session_manager.load_tree().await?;
    let rendered = crate::tree_display::render_tree_ascii(&tree);
    session.renderer.print_notice(&format!(
        "\nConversation Tree (Session: {}):\n{rendered}\n",
        engine.session_manager.session_id
    ));
    Ok(())
}

pub(crate) async fn maybe_summarize_abandoned(
    abandoned: &[&rho_harness_core::session::tree::TreeNodeData],
    engine: &AgentEngine,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Option<String> {
    if !has_assistant_turn(abandoned) {
        return None;
    }
    let answer = crate::repl::prompt::read_answer(reader, writer, SUMMARY_PROMPT)?;
    summarize_abandoned(engine, abandoned, should_summarize_branch(&answer)).await
}

async fn apply_branch_switch(
    leaf_id: &str,
    summary: Option<&str>,
    old_leaf: &str,
    engine: &mut AgentEngine,
    renderer: &crate::ui::TerminalRenderer,
) -> Result<()> {
    let _ = engine.switch_branch(Some(leaf_id.to_string())).await?;
    if let Some(s) = summary
        && let Err(err) = engine.session_manager.append_branch_summary(s, old_leaf).await
    {
        renderer.print_notice(&format!("  [Warning: branch summary was not saved: {err}]\n"));
    }
    Ok(())
}

pub async fn switch_active_branch(leaf_id: String, session: &mut ReplSession, engine: &mut AgentEngine) -> Result<()> {
    let (old_leaf, tree) = load_branch_context(engine).await?;
    let (abandoned, _) = tree.branch_divergence(&old_leaf, &leaf_id);
    let summary = if std::io::stdin().is_terminal() {
        maybe_summarize_abandoned(&abandoned, engine, &mut std::io::stdin().lock(), &mut std::io::stdout()).await
    } else {
        None
    };

    apply_branch_switch(&leaf_id, summary.as_deref(), &old_leaf, engine, &session.renderer).await?;
    *engine = engine
        .rebuild(session.config.clone(), session.auth_store.clone())
        .await?;
    session
        .renderer
        .print_notice(&format!("  [Switched active branch to {leaf_id}]\n"));
    Ok(())
}

async fn load_branch_context(engine: &AgentEngine) -> Result<(String, rho_harness_core::session::tree::SessionTree)> {
    let old_leaf = engine.session_manager.active_leaf_id().await?.unwrap_or_default();
    let tree = engine.session_manager.load_tree().await?;
    Ok((old_leaf, tree))
}

pub(crate) async fn fork_session(
    session: &ReplSession,
    engine: &AgentEngine,
    target: Option<crate::repl::commands::ForkTarget>,
    id: Option<&str>,
) -> Result<()> {
    let message = session_ops::fork(&session.config.sessions_dir, engine, target, id).await?;
    session.renderer.print_status(&message);
    Ok(())
}

pub(crate) async fn clone_session(session: &ReplSession, engine: &AgentEngine) -> Result<()> {
    let cloned = engine
        .session_manager
        .clone_session(&session.config.sessions_dir)
        .await?;
    session
        .renderer
        .print_status(&format!("Cloned session: {}", cloned.session_id));
    Ok(())
}

pub(crate) async fn handle_session_branching_result(
    cmd_res: &CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<bool> {
    match cmd_res {
        CommandResult::Tree | CommandResult::OpenTreeSelector => show_tree(session, engine).await?,
        CommandResult::SwitchBranch { leaf_id } => switch_active_branch(leaf_id.clone(), session, engine).await?,
        CommandResult::ForkSession {
            target,
            turn_or_node_id,
        } => {
            fork_session(session, engine, *target, turn_or_node_id.as_deref()).await?;
        }
        CommandResult::CloneSession => {
            clone_session(session, engine).await?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

pub(crate) fn show_session_summaries(session: &ReplSession) -> Result<()> {
    let mut shown = 0;
    for s in rho_harness_core::session::list_session_summaries(&session.config.sessions_dir)? {
        if s.turn_count == 0 && s.name.is_none() {
            continue;
        }
        shown += 1;
        session
            .renderer
            .print_notice(&format!("  - {} ({}): {}\n", s.session_id, s.turn_count, s.preview));
    }
    if shown == 0 {
        session.renderer.print_notice("  No previous sessions found.\n");
    }
    Ok(())
}

async fn resume_and_report(session_id: &str, session: &mut ReplSession, engine: &mut AgentEngine) {
    let message = match session_ops::resume(session, engine, session_id).await {
        Ok(resumed) => session_ops::resumed_message(&resumed),
        Err(err) => session_ops::resume_failed_message(session_id, &err),
    };
    session.renderer.print_status(&message);
}

pub(crate) async fn handle_session_manage_result(
    cmd_res: &CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<bool> {
    match cmd_res {
        CommandResult::ResumeSession { session_id } => resume_and_report(session_id, session, engine).await,
        CommandResult::OpenSessionSelector => {
            show_session_summaries(session)?;
        }
        CommandResult::NameSession { name } => {
            let message = session_ops::name_session(engine, name).await?;
            session.renderer.print_status(&message);
        }
        CommandResult::Rewind { turn } => {
            let message = session_ops::rewind(engine, *turn).await;
            session.renderer.print_status(&message);
        }
        _ => return Ok(false),
    }
    Ok(true)
}

pub(crate) async fn rebuild_engine_on_auth(session: &mut ReplSession, engine: &mut AgentEngine) -> Result<()> {
    *engine = engine
        .rebuild(session.config.clone(), session.auth_store.clone())
        .await?;
    session.config = engine.config.clone();
    session.refresh_model_catalog();
    crate::repl::report_mcp_diagnostics(session, engine);
    Ok(())
}

async fn login(provider: Option<&str>, session: &mut ReplSession, engine: &mut AgentEngine) -> Result<()> {
    crate::cli::login_provider(provider, false, &session.config, &mut session.auth_store).await?;
    rebuild_engine_on_auth(session, engine).await
}

async fn logout(provider: Option<&str>, session: &mut ReplSession, engine: &mut AgentEngine) -> Result<()> {
    crate::cli::logout_provider(provider, &session.config, &mut session.auth_store)?;
    rebuild_engine_on_auth(session, engine).await
}

pub(crate) async fn handle_auth_actions(
    cmd_res: &CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<bool> {
    match cmd_res {
        CommandResult::Login { provider } => login(provider.as_deref(), session, engine).await?,
        CommandResult::OpenLoginSelector => login(None, session, engine).await?,
        CommandResult::Logout { provider } => logout(provider.as_deref(), session, engine).await?,
        CommandResult::Reload => *engine = session.reload_engine(engine).await?,
        CommandResult::EnterWorktree { branch } => {
            let message = session_ops::enter_worktree(engine, branch.as_deref()).await;
            session.renderer.print_status(&message);
        }
        _ => return Ok(false),
    }
    Ok(true)
}

pub(crate) async fn handle_config_auth_result(
    cmd_res: &CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<bool> {
    if handle_auth_actions(cmd_res, session, engine).await? {
        return Ok(true);
    }
    match cmd_res {
        CommandResult::ClearContext => {
            let next = session.new_engine(None).await?;
            engine.switch_to(next);
        }
        CommandResult::ModelChanged { route } => {
            if switch_session_model(route, session, engine).await {
                print_session_model(session);
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

pub(crate) async fn compact_context(session: &ReplSession, engine: &AgentEngine, instructions: Option<&str>) {
    session.renderer.print_status(session_ops::COMPACTING);
    let message = session_ops::compact(engine, instructions).await;
    session.renderer.print_status(&message);
}

fn handle_expanded_prompt(session: &ReplSession, text: String) -> DispatchOutcome {
    session.renderer.print_notice("  [Expanded template]\n");
    session.renderer.print_user_block(&text);
    session.renderer.write_output("\n");
    DispatchOutcome::RunTurn(text)
}

/// Runs `cmd_res` when it changes the session, branch, engine or auth; false when it is none of those.
pub(crate) async fn handle_command_group(
    cmd_res: &CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<bool> {
    Ok(handle_session_branching_result(cmd_res, session, engine).await?
        || handle_session_manage_result(cmd_res, session, engine).await?
        || handle_config_auth_result(cmd_res, session, engine).await?)
}

pub(crate) async fn handle_terminal_result(
    cmd_res: CommandResult,
    session: &ReplSession,
    engine: &AgentEngine,
) -> Result<DispatchOutcome> {
    match cmd_res {
        CommandResult::Exit => Ok(DispatchOutcome::Break),
        CommandResult::Compact { instructions } => {
            compact_context(session, engine, instructions.as_deref()).await;
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::Recap => {
            run_line_mode_recap(session, engine).await;
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::ExpandedPrompt { text } => Ok(handle_expanded_prompt(session, text)),
        CommandResult::RunWorkflow(launch) => {
            run_workflow_plain(session, engine, launch).await;
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::Btw { question } => {
            handle_line_btw(session, engine, question).await;
            Ok(DispatchOutcome::Continue)
        }
        _ => Ok(DispatchOutcome::Continue),
    }
}

async fn handle_line_btw(session: &ReplSession, engine: &AgentEngine, question: Option<String>) {
    let Some(prompt) = question else {
        session.renderer.print_notice("  Usage: /btw <question>\n");
        return;
    };
    let req = rho_engine::engine::EphemeralTurnRequest {
        prompt,
        ..Default::default()
    };
    match engine.run_ephemeral_turn(req, None).await {
        Ok(response) => session.renderer.print_notice(&format!("\n{}\n\n", response.text)),
        Err(err) => session.renderer.print_notice(&format!("  /btw failed: {err}\n")),
    }
}

async fn run_line_mode_recap(session: &ReplSession, engine: &AgentEngine) {
    match session_ops::recap(engine).await {
        RecapOutcome::Text(text) => {
            let rendered = crate::ui::interactive::format_recap_notice(&text, 80, &session.renderer.theme);
            session.renderer.print_notice(&rendered);
        }
        RecapOutcome::Status(message) => session.renderer.print_status(&message),
    }
}

async fn run_workflow_plain(session: &ReplSession, engine: &AgentEngine, launch: WorkflowLaunch) {
    let (progress, mut chunks) = crate::repl::live::workflow_runner::progress_channel();
    let cancel = crate::engine::runner::CancellationSignal::default();
    let runner = match engine.workflow_runner(progress, cancel).await {
        Ok(runner) => runner,
        Err(e) => {
            session.renderer.print_notice(&format!("  /workflow: {e}\n"));
            return;
        }
    };
    let label = launch.label().to_string();
    let run = launch.run(&runner);
    tokio::pin!(run);
    let result = loop {
        tokio::select! {
            result = &mut run => break result,
            Some(chunk) = chunks.recv() => session.renderer.print_notice(&format!("  {chunk}")),
        }
    };
    if result.started {
        engine.hand_off_workflow_report(&label, &result.report);
    }
    session.renderer.print_notice(&format!("\n{}\n", result.report));
}

pub async fn handle_command_result(
    cmd_res: CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<DispatchOutcome> {
    match handle_command_group(&cmd_res, session, engine).await {
        Ok(true) => Ok(DispatchOutcome::Continue),
        Ok(false) => handle_terminal_result(cmd_res, session, engine).await,
        Err(error) => {
            session.renderer.write_output(&format!("\nError: {error}\n"));
            Ok(DispatchOutcome::Continue)
        }
    }
}
