use super::commands::handle_tree_commands;
use super::types::{BranchSwitchContext, LiveCommandContext, SessionCommandIo};
use crate::error::Result;
use crate::repl::branch_summary::{SUMMARY_PROMPT, abandoned_messages, has_assistant_turn, should_summarize_branch};
use crate::repl::commands::{CommandResult, ForkTarget};
use crate::ui::interactive::TerminalBackend;

pub(crate) async fn handle_fork_session(
    ctx: &mut LiveCommandContext<'_, '_>,
    target: Option<ForkTarget>,
    turn_or_node_id: &Option<String>,
) -> Result<()> {
    let forked_id = ctx
        .engine
        .session_manager
        .fork_session(&ctx.session.config.sessions_dir, turn_or_node_id.as_deref())
        .await?
        .session_id;
    let msg = crate::repl::live::mux::fork_status_message(&forked_id, target, &ctx.engine.base_dir);
    ctx.session.renderer.print_status(&msg);
    Ok(())
}

pub(crate) async fn handle_clone_session(ctx: &mut LiveCommandContext<'_, '_>) -> Result<()> {
    let cloned = ctx
        .engine
        .session_manager
        .clone_session(&ctx.session.config.sessions_dir)
        .await?;
    ctx.session
        .renderer
        .print_status(&format!("Cloned session: {}", cloned.session_id));
    Ok(())
}

pub(crate) async fn handle_resume_session<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    session_id: &str,
    io: &mut SessionCommandIo<'_, B>,
) -> Result<()> {
    let mut missing_workspace: Option<std::path::PathBuf> = None;
    if let Ok(Some(saved)) =
        rho_harness_core::session::SessionManager::cwd_for_session_async(&ctx.session.config.sessions_dir, session_id)
            .await
    {
        if saved.is_dir() {
            let _ = std::env::set_current_dir(&saved);
        } else {
            missing_workspace = Some(saved);
        }
    }
    let next = ctx.session.new_engine(Some(session_id)).await?;
    ctx.engine.switch_to(next);
    ctx.session.sync_engine_model(ctx.engine).await;
    if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
        let _ = crate::repl::live::navigation::hydrate_session_transcript(io.controller, &tree);
        if !tree.is_empty() && ctx.session.config.recap.on_resume {
            ctx.session.pending_resume_recap = true;
        }
    }
    crate::repl::live::navigation::update_footer(io.controller.state_mut(), ctx.session, ctx.engine);
    let suffix = missing_workspace
        .as_ref()
        .map(|p| format!(" (previous workspace {} no longer exists)", p.display()))
        .unwrap_or_default();
    ctx.session
        .renderer
        .print_status(&format!("Resumed session {session_id}{suffix}"));
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SessionSwitchTarget<'a> {
    Fork(Option<ForkTarget>, &'a Option<String>),
    Clone,
    Resume(&'a str),
}

pub(crate) fn parse_switch_target(result: &CommandResult) -> Option<SessionSwitchTarget<'_>> {
    match result {
        CommandResult::ForkSession {
            target,
            turn_or_node_id,
        } => Some(SessionSwitchTarget::Fork(*target, turn_or_node_id)),
        CommandResult::CloneSession => Some(SessionSwitchTarget::Clone),
        CommandResult::ResumeSession { session_id } => Some(SessionSwitchTarget::Resume(session_id.as_str())),
        _ => None,
    }
}

pub(crate) async fn switch_session_result<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<()> {
    let Some(target) = parse_switch_target(result) else {
        return Ok(());
    };
    match target {
        SessionSwitchTarget::Fork(target, id) => handle_fork_session(ctx, target, id).await,
        SessionSwitchTarget::Clone => handle_clone_session(ctx).await,
        SessionSwitchTarget::Resume(id) => handle_resume_session(ctx, id, io).await,
    }
}

pub(crate) async fn handle_switch_session_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<()> {
    if let CommandResult::SwitchBranch { leaf_id } = result {
        return switch_branch_for_command(ctx, io, leaf_id).await;
    }
    switch_session_result(ctx, io, result).await
}

pub(crate) fn is_tree_arm(result: &CommandResult) -> bool {
    matches!(result, CommandResult::OpenTreeSelector | CommandResult::Tree)
}

pub(crate) fn is_session_switch_arm(result: &CommandResult) -> bool {
    matches!(
        result,
        CommandResult::SwitchBranch { .. }
            | CommandResult::ForkSession { .. }
            | CommandResult::CloneSession
            | CommandResult::ResumeSession { .. }
    )
}

pub(crate) async fn dispatch_session_result<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<bool> {
    if is_tree_arm(result) {
        handle_tree_commands(ctx, io, result).await?;
        return Ok(true);
    }
    if is_session_switch_arm(result) {
        handle_switch_session_command(ctx, io, result).await?;
        return Ok(true);
    }
    handle_session_diagnostics(ctx, io, result).await
}

pub(crate) async fn switch_branch_for_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    leaf_id: &str,
) -> Result<()> {
    handle_switch_branch(
        BranchSwitchContext {
            session: ctx.session,
            engine: ctx.engine,
            controller: io.controller,
            input: io.input,
        },
        leaf_id.to_string(),
    )
    .await
}

pub(crate) async fn handle_session_diagnostics<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<bool> {
    match result {
        CommandResult::OpenSessionSelector => {
            crate::repl::live::modal::open_session_selector(
                &ctx.session.config.sessions_dir,
                &ctx.engine.session_manager.session_id,
                io.controller,
            )
            .await;
            io.controller.redraw()?;
        }
        CommandResult::NameSession { name } => {
            ctx.engine.session_manager.set_session_name(name).await?;
            ctx.session.renderer.print_status(&format!("Session name: \"{name}\""));
        }
        CommandResult::Rewind { turn } => {
            let retained_count = ctx.engine.rewind_to_turn(*turn).await?;
            ctx.session.renderer.print_notice(&format!(
                "  [Rewound context to Turn {turn} ({retained_count} messages retained)]\n"
            ));
        }
        _ => return Ok(false),
    }
    Ok(true)
}

pub(crate) async fn handle_session_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    mut io: SessionCommandIo<'_, B>,
    result: CommandResult,
) -> Result<bool> {
    dispatch_session_result(ctx, &mut io, &result).await
}

fn confirm_branch_summary() -> bool {
    use std::io::Write;
    print!("{SUMMARY_PROMPT}");
    std::io::stdout().flush().ok();
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .is_ok_and(|_| should_summarize_branch(&input))
}

async fn summarize_if_confirmed(ctx: &mut BranchSwitchContext<'_, '_, '_, impl TerminalBackend>) -> Option<()> {
    let mut paused = ctx.input.pause().ok()?;
    paused.drain();
    ctx.controller.suspend().ok()?;
    let confirmed = confirm_branch_summary();
    let controller_res = ctx.controller.resume();
    let input_res = paused.resume();
    ctx.input.drain();
    controller_res.ok()?;
    input_res.ok()?;
    confirmed.then_some(())
}

async fn maybe_summarize_abandoned(
    ctx: &mut BranchSwitchContext<'_, '_, '_, impl TerminalBackend>,
    abandoned: &[&rho_harness_core::session::tree::TreeNodeData],
) -> Option<String> {
    if !has_assistant_turn(abandoned) || !ctx.session.renderer.has_interactive_ui() {
        return None;
    }
    summarize_if_confirmed(ctx).await?;
    Some(ctx.engine.summarize_branch(&abandoned_messages(abandoned)).await)
}

async fn rebuild_and_hydrate(
    ctx: &mut BranchSwitchContext<'_, '_, '_, impl TerminalBackend>,
    leaf_id: &str,
) -> Result<()> {
    *ctx.engine = ctx
        .engine
        .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
        .await?;
    if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
        let _ = crate::repl::live::navigation::hydrate_session_transcript(ctx.controller, &tree);
    }
    ctx.session
        .renderer
        .print_status(&format!("Switched active branch to {leaf_id}"));
    Ok(())
}

async fn switch_and_hydrate(
    ctx: &mut BranchSwitchContext<'_, '_, '_, impl TerminalBackend>,
    leaf_id: &str,
    summary: Option<String>,
    old_leaf: &str,
) -> Result<()> {
    ctx.engine.switch_branch(Some(leaf_id.to_string())).await?;
    if let Some(summary) = summary.as_deref()
        && let Err(err) = ctx
            .engine
            .session_manager
            .append_branch_summary(summary, old_leaf)
            .await
    {
        ctx.controller
            .set_system_message(format!("Warning: branch summary was not saved: {err}"));
    }
    rebuild_and_hydrate(ctx, leaf_id).await
}

pub(crate) async fn handle_switch_branch<B: TerminalBackend>(
    mut ctx: BranchSwitchContext<'_, '_, '_, B>,
    leaf_id: String,
) -> Result<()> {
    let old_leaf = ctx.engine.session_manager.active_leaf_id().await?.unwrap_or_default();
    let tree = ctx.engine.session_manager.load_tree().await?;
    let (abandoned, _) = tree.branch_divergence(&old_leaf, &leaf_id);
    let summary = maybe_summarize_abandoned(&mut ctx, &abandoned).await;
    switch_and_hydrate(&mut ctx, &leaf_id, summary, &old_leaf).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_switch_target_mappings() {
        let fork_target = CommandResult::ForkSession {
            target: Some(ForkTarget::Pane),
            turn_or_node_id: Some("turn-1".into()),
        };
        assert_eq!(
            parse_switch_target(&fork_target),
            Some(SessionSwitchTarget::Fork(
                Some(ForkTarget::Pane),
                &Some("turn-1".into())
            ))
        );
        let fork_none = CommandResult::ForkSession {
            target: None,
            turn_or_node_id: None,
        };
        assert_eq!(
            parse_switch_target(&fork_none),
            Some(SessionSwitchTarget::Fork(None, &None))
        );
        assert_eq!(
            parse_switch_target(&CommandResult::CloneSession),
            Some(SessionSwitchTarget::Clone)
        );
        assert_eq!(
            parse_switch_target(&CommandResult::ResumeSession {
                session_id: "sess-abc".into()
            }),
            Some(SessionSwitchTarget::Resume("sess-abc"))
        );
        assert_eq!(parse_switch_target(&CommandResult::Continue), None);
    }

    #[test]
    fn test_is_tree_and_switch_arms() {
        assert!(is_tree_arm(&CommandResult::Tree));
        assert!(is_tree_arm(&CommandResult::OpenTreeSelector));
        assert!(!is_tree_arm(&CommandResult::CloneSession));

        assert!(is_session_switch_arm(&CommandResult::CloneSession));
        assert!(is_session_switch_arm(&CommandResult::ForkSession {
            target: None,
            turn_or_node_id: None
        }));
        assert!(is_session_switch_arm(&CommandResult::ResumeSession {
            session_id: "s1".into()
        }));
        assert!(is_session_switch_arm(&CommandResult::SwitchBranch {
            leaf_id: "b1".into()
        }));
        assert!(!is_session_switch_arm(&CommandResult::Continue));
    }
}
