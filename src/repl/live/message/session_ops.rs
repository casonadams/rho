use super::commands::handle_tree_commands;
use super::types::{BranchSwitchContext, LiveCommandContext, SessionCommandIo};
use crate::error::Result;
use crate::repl::commands::CommandResult;
use crate::ui::interactive::TerminalBackend;
use rho_harness_core::session::TreeNodeKind;

pub(crate) async fn handle_fork_session(
    ctx: &mut LiveCommandContext<'_, '_>,
    turn_or_node_id: &Option<String>,
) -> Result<()> {
    let forked = ctx
        .engine
        .session_manager
        .fork_session(&ctx.session.config.sessions_dir, turn_or_node_id.as_deref())
        .await?;
    ctx.session
        .renderer
        .print_status(&format!("Forked session: {}", forked.session_id));
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
    *ctx.engine = crate::platform::agent_engine(
        ctx.session.config.clone(),
        ctx.session.auth_store.clone(),
        Some(session_id),
    )
    .await?;
    if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
        let _ = crate::repl::live::navigation::hydrate_session_transcript(io.controller, &tree, io.history);
    }
    ctx.session
        .renderer
        .print_status(&format!("Resumed session {session_id}"));
    Ok(())
}

pub(crate) async fn switch_session_result<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<()> {
    match result {
        CommandResult::ForkSession { turn_or_node_id } => handle_fork_session(ctx, turn_or_node_id).await?,
        CommandResult::CloneSession => handle_clone_session(ctx).await?,
        CommandResult::ResumeSession { session_id } => handle_resume_session(ctx, session_id, io).await?,
        _ => {}
    }
    Ok(())
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
            history: io.history,
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
            crate::repl::live::modal::open_session_selector(&ctx.session.config.sessions_dir, io.controller);
            io.controller.redraw()?;
        }
        CommandResult::NameSession { name } => {
            ctx.engine.session_manager.set_session_name(name).await?;
            ctx.session.renderer.print_status(&format!("Session name: \"{name}\""));
        }
        CommandResult::Rewind { turn } => {
            let retained_count = ctx.engine.session_manager.rewind_to_turn(*turn).await?;
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
    print!("Summarize discoveries from abandoned branch before switching? [Y/n]: ");
    std::io::stdout().flush().ok();
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_ok() {
        let trimmed = input.trim().to_lowercase();
        trimmed.is_empty() || trimmed == "y" || trimmed == "yes"
    } else {
        false
    }
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

fn abandoned_messages(abandoned: &[&rho_harness_core::session::tree::TreeNodeData]) -> Vec<rig::message::Message> {
    abandoned.iter().flat_map(|n| n.messages.clone()).collect()
}

async fn maybe_summarize_abandoned(
    ctx: &mut BranchSwitchContext<'_, '_, '_, impl TerminalBackend>,
    abandoned: &[&rho_harness_core::session::tree::TreeNodeData],
) -> Option<String> {
    let has_assistant = abandoned.iter().any(|n| n.kind == TreeNodeKind::AssistantTurn);
    if !has_assistant || !ctx.session.renderer.has_interactive_ui() {
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
        let _ = crate::repl::live::navigation::hydrate_session_transcript(ctx.controller, &tree, ctx.history);
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
    ctx.engine
        .session_manager
        .switch_branch(Some(leaf_id.to_string()))
        .await?;
    if let Some(summary) = summary.as_deref() {
        let _ = ctx
            .engine
            .session_manager
            .append_branch_summary(summary, old_leaf)
            .await;
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
