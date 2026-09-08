use super::tree::{show_tree, switch_active_branch};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::CommandResult;

pub enum DispatchOutcome {
    Continue,
    Break,
    RunTurn(String),
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
            fork_or_clone_session(Some(turn_or_node_id.as_deref()), (false, session), engine).await?;
        }
        CommandResult::CloneSession => {
            fork_or_clone_session(None, (true, session), engine).await?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

async fn fork_or_clone_session(
    fork: Option<Option<&str>>,
    (clone, session): (bool, &ReplSession),
    engine: &AgentEngine,
) -> Result<()> {
    if let Some(id) = fork {
        let forked = engine
            .session_manager
            .fork_session(&session.config.sessions_dir, id)
            .await?;
        session
            .renderer
            .print_status(&format!("Forked session: {}", forked.session_id));
    } else if clone {
        let cloned = engine
            .session_manager
            .clone_session(&session.config.sessions_dir)
            .await?;
        session
            .renderer
            .print_status(&format!("Cloned session: {}", cloned.session_id));
    }
    Ok(())
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
        CommandResult::OpenSessionSelector => show_session_summaries(session)?,
        CommandResult::ResumeSession { session_id } => {
            *engine =
                crate::platform::agent_engine(session.config.clone(), session.auth_store.clone(), Some(session_id))
                    .await?;
            session.renderer.print_status(&format!("Resumed session {session_id}"));
        }
        CommandResult::NameSession { name } => {
            engine.session_manager.set_session_name(name).await?;
            session.renderer.print_status(&format!("Session name: \"{name}\""));
        }
        CommandResult::Rewind { turn } => {
            let count = engine.session_manager.rewind_to_turn(*turn).await?;
            session.renderer.print_notice(&format!(
                "  [Rewound context to Turn {turn} ({count} messages retained)]\n"
            ));
        }
        _ => return Ok(false),
    }
    Ok(true)
}

async fn handle_model_change(
    (new_model, new_provider): (&str, Option<&str>),
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) {
    session.config.model = new_model.to_string();
    if let Some(p) = new_provider {
        session.config.provider = p.to_string();
    }
    if let Err(err) = engine.switch_model(new_model, &session.config.provider).await {
        session
            .renderer
            .print_notice(&format!("  Warning: Could not switch model: {err}\n"));
    }
}

async fn rebuild_engine_on_auth(session: &mut ReplSession, engine: &mut AgentEngine) -> Result<()> {
    *engine = engine
        .rebuild(session.config.clone(), session.auth_store.clone())
        .await?;
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
            handle_model_change((new_model, new_provider.as_deref()), session, engine).await;
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
