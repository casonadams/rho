use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::CommandResult;

pub enum DispatchOutcome {
    Continue,
    Break,
    RunTurn(String),
}

pub async fn show_tree(session: &ReplSession, engine: &AgentEngine) -> Result<()> {
    let tree = engine.session_manager.load_tree().await?;
    let rendered = crate::ui::interactive::tree_view::render_tree_ascii(&tree);
    session.renderer.print_notice(&format!(
        "\nConversation Tree (Session: {}):\n{rendered}\n",
        engine.session_manager.session_id
    ));
    Ok(())
}

async fn maybe_summarize_abandoned(
    abandoned: &[&rho_harness_core::session::tree::TreeNodeData],
    engine: &AgentEngine,
    has_ui: bool,
) -> Option<String> {
    let has_assistant = abandoned
        .iter()
        .any(|n| n.kind == rho_harness_core::session::TreeNodeKind::AssistantTurn);
    if !has_assistant || !has_ui {
        return None;
    }
    use std::io::Write;
    print!("Summarize discoveries from abandoned branch before switching? [Y/n]: ");
    std::io::stdout().flush().ok()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok()?;
    let trimmed = input.trim().to_lowercase();
    if !trimmed.is_empty() && trimmed != "y" && trimmed != "yes" {
        return None;
    }
    let messages: Vec<_> = abandoned.iter().flat_map(|n| n.messages.clone()).collect();
    Some(engine.summarize_branch(&messages).await)
}

async fn apply_branch_switch(
    leaf_id: &str,
    summary: Option<&str>,
    old_leaf: &str,
    engine: &mut AgentEngine,
) -> Result<()> {
    let _ = engine.session_manager.switch_branch(Some(leaf_id.to_string())).await?;
    if let Some(s) = summary {
        let _ = engine.session_manager.append_branch_summary(s, old_leaf).await;
    }
    Ok(())
}

pub async fn switch_active_branch(leaf_id: String, session: &mut ReplSession, engine: &mut AgentEngine) -> Result<()> {
    let (old_leaf, tree) = load_branch_context(engine).await?;
    let (abandoned, _) = tree.branch_divergence(&old_leaf, &leaf_id);
    let summary = maybe_summarize_abandoned(&abandoned, engine, session.renderer.has_interactive_ui()).await;

    apply_branch_switch(&leaf_id, summary.as_deref(), &old_leaf, engine).await?;
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

async fn fork_session(session: &ReplSession, engine: &AgentEngine, id: Option<&str>) -> Result<()> {
    let forked = engine
        .session_manager
        .fork_session(&session.config.sessions_dir, id)
        .await?;
    session
        .renderer
        .print_status(&format!("Forked session: {}", forked.session_id));
    Ok(())
}

async fn clone_session(session: &ReplSession, engine: &AgentEngine) -> Result<()> {
    let cloned = engine
        .session_manager
        .clone_session(&session.config.sessions_dir)
        .await?;
    session
        .renderer
        .print_status(&format!("Cloned session: {}", cloned.session_id));
    Ok(())
}

async fn handle_session_branching_result(
    cmd_res: &CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<bool> {
    match cmd_res {
        CommandResult::Tree | CommandResult::OpenTreeSelector => show_tree(session, engine).await?,
        CommandResult::SwitchBranch { leaf_id } => switch_active_branch(leaf_id.clone(), session, engine).await?,
        CommandResult::ForkSession { turn_or_node_id } => {
            fork_session(session, engine, turn_or_node_id.as_deref()).await?;
        }
        CommandResult::CloneSession => {
            clone_session(session, engine).await?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn show_session_summaries(session: &ReplSession) -> Result<()> {
    for s in rho_harness_core::session::list_session_summaries(&session.config.sessions_dir)? {
        session
            .renderer
            .print_notice(&format!("  - {} ({}): {}\n", s.session_id, s.turn_count, s.preview));
    }
    Ok(())
}

async fn handle_session_manage_result(
    cmd_res: &CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<bool> {
    match cmd_res {
        CommandResult::ResumeSession { session_id } => {
            let next_config = session.config.clone();
            *engine = crate::platform::agent_engine(next_config, session.auth_store.clone(), Some(session_id)).await?;
            session.config = engine.config.clone();
            session.resume_id = Some(session_id.clone());
            session.renderer.print_status(&format!("Resumed session: {session_id}"));
        }
        CommandResult::OpenSessionSelector => {
            show_session_summaries(session)?;
        }
        CommandResult::NameSession { name } => {
            engine.session_manager.set_session_name(name).await?;
            session.renderer.print_status(&format!("Session name: {name}"));
        }
        CommandResult::Rewind { turn } => match engine.session_manager.rewind_to_turn(*turn).await {
            Ok(count) => session
                .renderer
                .print_status(&format!("Rewound to turn {turn} ({count} messages in context)")),
            Err(e) => session.renderer.print_status(&format!("Rewind failed: {e}")),
        },
        _ => return Ok(false),
    }
    Ok(true)
}

async fn rebuild_engine_on_auth(session: &mut ReplSession, engine: &mut AgentEngine) -> Result<()> {
    let next_config = session.config.clone();
    *engine =
        crate::platform::agent_engine(next_config, session.auth_store.clone(), session.resume_id.as_deref()).await?;
    session.config = engine.config.clone();
    Ok(())
}

async fn try_login(provider: Option<&str>, session: &mut ReplSession, engine: &mut AgentEngine) -> Result<bool> {
    let ok = crate::cli::login_provider(provider, &session.config, &mut session.auth_store)
        .await
        .is_ok();
    if ok {
        rebuild_engine_on_auth(session, engine).await?;
    }
    Ok(ok)
}

async fn try_logout(provider: Option<&str>, session: &mut ReplSession, engine: &mut AgentEngine) -> Result<bool> {
    let ok = crate::cli::logout_provider(provider, &session.config, &mut session.auth_store).is_ok();
    if ok {
        rebuild_engine_on_auth(session, engine).await?;
    }
    Ok(ok)
}

async fn try_login_action(
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

async fn handle_auth_actions(
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
        _ => Ok(false),
    }
}

async fn handle_model_change(
    new_model: &str,
    new_provider: Option<&str>,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) {
    session.config.model = new_model.to_string();
    if let Some(prov) = new_provider {
        session.config.provider = prov.to_string();
    }
    let prov = new_provider.unwrap_or(&session.config.provider);
    let _ = engine.switch_model(new_model, prov).await;
}

async fn handle_config_auth_result(
    cmd_res: &CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<bool> {
    if handle_auth_actions(cmd_res, session, engine).await? {
        return Ok(true);
    }
    match cmd_res {
        CommandResult::ClearContext => {
            *engine = crate::platform::agent_engine(session.config.clone(), session.auth_store.clone(), None).await?;
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

fn print_compaction_stats(session: &ReplSession, stats: &rho_engine::engine::CompactionStats) {
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

async fn handle_command_group(cmd_res: &CommandResult, session: &mut ReplSession, engine: &mut AgentEngine) -> bool {
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

async fn handle_terminal_result(
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
        CommandResult::ExpandedPrompt { text } => Ok(handle_expanded_prompt(session, text)),
        _ => Ok(DispatchOutcome::Continue),
    }
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
