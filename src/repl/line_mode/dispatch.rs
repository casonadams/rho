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

pub async fn handle_command_result(
    cmd_res: CommandResult,
    session: &mut ReplSession,
    engine: &mut AgentEngine,
) -> Result<DispatchOutcome> {
    match cmd_res {
        CommandResult::Exit => Ok(DispatchOutcome::Break),
        CommandResult::OpenModelSelector | CommandResult::OpenSettingsSelector => {
            // In legacy reedline mode, fallback to command prompt
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::ClearContext => {
            *engine = crate::platform::agent_engine(session.config.clone(), session.auth_store.clone(), None).await?;
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::ModelChanged {
            new_model,
            new_provider,
        } => {
            session.config.model = new_model;
            if let Some(provider) = new_provider {
                session.config.provider = provider;
            }
            *engine = engine
                .rebuild(session.config.clone(), session.auth_store.clone())
                .await?;
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::Login { provider } => {
            match crate::cli::login_provider(provider.as_deref(), &session.config, &mut session.auth_store).await {
                Ok(()) => {
                    *engine = engine
                        .rebuild(session.config.clone(), session.auth_store.clone())
                        .await?;
                }
                Err(crate::error::AppError::Cancelled(_)) => {}
                Err(err) => session.renderer.print_notice(&format!("  Login failed: {err}\n")),
            }
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::Reload => {
            *engine = session.reload_engine(engine).await?;
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::Compact { instructions } => {
            compact_context(session, engine, instructions.as_deref()).await;
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::Tree | CommandResult::OpenTreeSelector => {
            show_tree(session, engine).await?;
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::SwitchBranch { leaf_id } => {
            switch_active_branch(leaf_id, session, engine).await?;
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::ForkSession { turn_or_node_id } => {
            let forked = engine
                .session_manager
                .fork_session(&session.config.sessions_dir, turn_or_node_id.as_deref())
                .await?;
            session
                .renderer
                .print_status(&format!("Forked session: {}", forked.session_id));
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::CloneSession => {
            let cloned = engine
                .session_manager
                .clone_session(&session.config.sessions_dir)
                .await?;
            session
                .renderer
                .print_status(&format!("Cloned session: {}", cloned.session_id));
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::OpenSessionSelector => {
            let summaries = rho_harness_core::session::list_session_summaries(&session.config.sessions_dir)?;
            for s in summaries {
                session
                    .renderer
                    .print_notice(&format!("  - {} ({}): {}\n", s.session_id, s.turn_count, s.preview));
            }
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::ResumeSession { session_id } => {
            *engine =
                crate::platform::agent_engine(session.config.clone(), session.auth_store.clone(), Some(&session_id))
                    .await?;
            session.renderer.print_status(&format!("Resumed session {session_id}"));
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::NameSession { name } => {
            engine.session_manager.set_session_name(&name).await?;
            session.renderer.print_status(&format!("Session name: \"{name}\""));
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::ExpandedPrompt { text } => {
            session.renderer.print_notice("  [Expanded template]\n");
            session.renderer.print_user_block(&text);
            session.renderer.write_output("\n");
            Ok(DispatchOutcome::RunTurn(text))
        }
        CommandResult::Rewind { turn } => {
            let count = engine.session_manager.rewind_to_turn(turn).await?;
            session.renderer.print_notice(&format!(
                "  [Rewound context to Turn {turn} ({count} messages retained)]\n"
            ));
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::Logout { provider } => {
            match crate::cli::logout_provider(provider.as_deref(), &session.config, &mut session.auth_store) {
                Ok(()) => {
                    *engine = engine
                        .rebuild(session.config.clone(), session.auth_store.clone())
                        .await?;
                }
                Err(crate::error::AppError::Cancelled(_)) => {}
                Err(err) => session.renderer.print_notice(&format!("  Logout failed: {err}\n")),
            }
            Ok(DispatchOutcome::Continue)
        }
        CommandResult::Continue => Ok(DispatchOutcome::Continue),
    }
}

pub(crate) async fn compact_context(session: &ReplSession, engine: &AgentEngine, instructions: Option<&str>) {
    session
        .renderer
        .print_notice("  [Compacting conversation context...]\n");
    match engine.compact_session(instructions).await {
        Ok(stats) => {
            let before = crate::ui::interactive::footer::format_tokens(stats.tokens_before as u64);
            let after = crate::ui::interactive::footer::format_tokens(stats.tokens_after as u64);
            let saved = crate::ui::interactive::footer::format_tokens(stats.saved_tokens as u64);
            session.renderer.print_notice(&format!(
                "  [Compacted context: {before} -> {after} tokens (saved {saved})]\n"
            ));
        }
        Err(err) => session
            .renderer
            .print_notice(&format!("  [Compaction failed: {err}]\n")),
    }
}
