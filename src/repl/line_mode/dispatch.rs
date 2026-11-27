use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::branch_summary::{SUMMARY_PROMPT, abandoned_messages, has_assistant_turn, should_summarize_branch};
use crate::repl::commands::CommandResult;
use crate::repl::commands::types::WorkflowLaunch;

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
    let forked = engine
        .session_manager
        .fork_session(&session.config.sessions_dir, id)
        .await?;
    let forked_id = forked.session_id.clone();
    drop(forked);
    if let Some(target) = target {
        let cwd = std::env::current_dir().unwrap_or_else(|_| engine.base_dir.clone());
        match crate::repl::live::mux::spawn_forked_session(target, &forked_id, &cwd) {
            Ok(label) => session
                .renderer
                .print_status(&format!("Forked session {forked_id} in {label}")),
            Err(err) => session.renderer.print_status(&err),
        }
    } else {
        session.renderer.print_status(&format!("Forked session: {forked_id}"));
    }
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

pub(crate) async fn try_login(
    provider: Option<&str>,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<bool> {
    let ok = crate::cli::login_provider(provider, false, &session.config, &mut session.auth_store)
        .await
        .is_ok();
    if ok {
        rebuild_engine_on_auth(session, engine).await?;
    }
    Ok(ok)
}

pub(crate) async fn try_logout(
    provider: Option<&str>,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<bool> {
    let ok = crate::cli::logout_provider(provider, &session.config, &mut session.auth_store).is_ok();
    if ok {
        rebuild_engine_on_auth(session, engine).await?;
    }
    Ok(ok)
}

pub(crate) async fn try_login_action(
    cmd_res: &CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<Option<bool>> {
    match cmd_res {
        CommandResult::Login { provider } => Ok(Some(try_login(provider.as_deref(), session, engine).await?)),
        CommandResult::OpenLoginSelector => Ok(Some(try_login(None, session, engine).await?)),
        _ => Ok(None),
    }
}

pub(crate) async fn handle_auth_actions(
    cmd_res: &CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<bool> {
    if let Some(handled) = try_login_action(cmd_res, session, engine).await? {
        return Ok(handled);
    }
    match cmd_res {
        CommandResult::Logout { provider } => try_logout(provider.as_deref(), session, engine).await,
        CommandResult::Reload => {
            *engine = session.reload_engine(engine).await?;
            Ok(true)
        }
        CommandResult::EnterWorktree { branch } => {
            match engine.enter_session_worktree(branch.as_deref()).await {
                Ok(created) => session.renderer.print_notice(&format!("  {}\n", created.summary())),
                Err(err) => session.renderer.print_notice(&format!("  Worktree failed: {err}\n")),
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Switches the engine to `new_model` and records it in the session config only if that worked;
/// both the line and live frontends use this so a failure is reported, never swallowed.
pub(crate) async fn switch_session_model(
    new_model: &str,
    new_provider: Option<&str>,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) {
    let provider = new_provider.map_or_else(|| session.config.provider.clone(), str::to_string);
    let result = engine.switch_model(new_model, &provider).await;
    if result.is_ok() {
        session.config.model = new_model.to_string();
        session.config.provider = provider;
    }
    crate::repl::live::navigation::report_model_update(session, engine, result, "Could not switch model");
}

pub(crate) async fn handle_model_change(
    new_model: &str,
    new_provider: Option<&str>,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) {
    switch_session_model(new_model, new_provider, session, engine).await;
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
        CommandResult::ModelChanged {
            new_model,
            new_provider,
        } => {
            handle_model_change(new_model, new_provider.as_deref(), session, engine).await;
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

pub(crate) async fn handle_command_group(
    cmd_res: &CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> bool {
    handle_session_branching_result(cmd_res, session, engine)
        .await
        .unwrap_or(false)
        || handle_session_manage_result(cmd_res, session, engine)
            .await
            .unwrap_or(false)
        || handle_config_auth_result(cmd_res, session, engine)
            .await
            .unwrap_or(false)
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
    if handle_command_group(&cmd_res, session, engine).await {
        return Ok(DispatchOutcome::Continue);
    }
    handle_terminal_result(cmd_res, session, engine).await
}
