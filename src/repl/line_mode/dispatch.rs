use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::branch_summary::{SUMMARY_PROMPT, abandoned_messages, has_assistant_turn, should_summarize_branch};
use crate::repl::commands::CommandResult;
use crate::repl::commands::types::WorkflowLaunch;
use crate::repl::model_switch::{print_session_model, switch_session_model};

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

fn prompt_abandoned_summary_confirmation() -> Option<bool> {
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout();
    let input = crate::repl::prompt::read_answer(&mut stdin, &mut stdout, SUMMARY_PROMPT)?;
    Some(should_summarize_branch(&input))
}

pub(crate) async fn maybe_summarize_abandoned(
    abandoned: &[&rho_harness_core::session::tree::TreeNodeData],
    engine: &AgentEngine,
    has_ui: bool,
) -> Option<String> {
    if !has_ui || !has_assistant_turn(abandoned) {
        return None;
    }
    if !prompt_abandoned_summary_confirmation().unwrap_or(false) {
        return None;
    }
    let messages = abandoned_messages(abandoned);
    Some(engine.summarize_branch(&messages).await)
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
    let summary = maybe_summarize_abandoned(&abandoned, engine, session.renderer.has_interactive_ui()).await;

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
    let forked_id = engine
        .session_manager
        .fork_session(&session.config.sessions_dir, id)
        .await?
        .session_id;
    let msg = crate::repl::live::mux::fork_status_message(&forked_id, target, &engine.base_dir);
    session.renderer.print_status(&msg);
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

pub(crate) async fn resume_session(
    session_id: &str,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<()> {
    if let Ok(Some(saved)) =
        rho_harness_core::session::SessionManager::cwd_for_session_async(&session.config.sessions_dir, session_id).await
        && saved.is_dir()
    {
        let _ = std::env::set_current_dir(&saved);
    }
    let next = session.new_engine(Some(session_id)).await?;
    engine.switch_to(next);
    session.config = engine.config.clone();
    session.resume_id = Some(session_id.to_string());
    session.renderer.print_status(&format!("Resumed session: {session_id}"));
    Ok(())
}

pub(crate) async fn name_session(name: &str, session: &ReplSession, engine: &AgentEngine) -> Result<()> {
    engine.session_manager.set_session_name(name).await?;
    session.renderer.print_status(&format!("Session name: {name}"));
    Ok(())
}

pub(crate) async fn rewind_session(turn: usize, session: &ReplSession, engine: &AgentEngine) {
    match engine.rewind_to_turn(turn).await {
        Ok(count) => session
            .renderer
            .print_status(&format!("Rewound to turn {turn} ({count} messages in context)")),
        Err(e) => session.renderer.print_status(&format!("Rewind failed: {e}")),
    }
}

pub(crate) async fn handle_session_manage_result(
    cmd_res: &CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<bool> {
    match cmd_res {
        CommandResult::ResumeSession { session_id } => {
            resume_session(session_id, session, engine).await?;
        }
        CommandResult::OpenSessionSelector => {
            show_session_summaries(session)?;
        }
        CommandResult::NameSession { name } => {
            name_session(name, session, engine).await?;
        }
        CommandResult::Rewind { turn } => {
            rewind_session(*turn, session, engine).await;
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
        CommandResult::EnterWorktree { branch } => match engine.enter_session_worktree(branch.as_deref()).await {
            Ok(created) => session.renderer.print_notice(&format!("  {}\n", created.summary())),
            Err(err) => session.renderer.print_notice(&format!("  Worktree failed: {err}\n")),
        },
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

pub(crate) fn print_compaction_stats(session: &ReplSession, stats: &rho_engine::engine::CompactionStats) {
    let before = crate::ui::interactive::footer::format_tokens(stats.tokens_before as u64);
    let after = crate::ui::interactive::footer::format_tokens(stats.tokens_after as u64);
    let saved = crate::ui::interactive::footer::format_tokens(stats.saved_tokens as u64);
    session.renderer.print_notice(&format!(
        "  [Compacted context: {before} -> {after} tokens (saved {saved})]\n"
    ));
}

pub(crate) async fn compact_context(session: &ReplSession, engine: &AgentEngine, instructions: Option<&str>) {
    session
        .renderer
        .print_notice("  [Compacting conversation context...]\n");
    match engine.compact_session(instructions).await {
        Ok(stats) => print_compaction_stats(session, &stats),
        Err(err) => {
            session
                .renderer
                .print_notice(&format!("  [Compaction failed: {err}]\n"));
        }
    }
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
    let cancel = crate::engine::runner::CancellationSignal::default();
    match engine.generate_recap(cancel).await {
        Ok(Some(text)) => {
            let rendered = crate::ui::interactive::format_recap_notice(&text, 80, &session.renderer.theme);
            session.renderer.print_notice(&rendered);
        }
        Ok(None) => session.renderer.print_status("Nothing to recap yet."),
        Err(err) => session.renderer.print_status(&format!("Recap failed: {err}")),
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
