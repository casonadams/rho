use super::branch::{BranchSwitchContext, handle_switch_branch};
use super::command::LiveCommandContext;
use crate::error::Result;
use crate::repl::commands::CommandResult;
use crate::repl::input_reader::TerminalInputReader;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub(super) struct SessionCommandIo<'a, B: TerminalBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub history: &'a mut InteractiveHistory,
    pub input: &'a mut TerminalInputReader,
}

async fn handle_tree_commands<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<()> {
    let tree = ctx.engine.session_manager.load_tree().await?;
    match result {
        CommandResult::OpenTreeSelector => {
            super::super::modal::open_tree_selector(&tree, io.controller);
            io.controller.redraw()?;
        }
        CommandResult::Tree => {
            let rendered = crate::ui::interactive::tree_view::render_tree_ascii(&tree);
            ctx.session.renderer.print_notice(&format!(
                "\nConversation Tree (Session: {}):\n{rendered}\n",
                ctx.engine.session_manager.session_id
            ));
        }
        _ => {}
    }
    Ok(())
}

async fn handle_fork_session(ctx: &mut LiveCommandContext<'_, '_>, turn_or_node_id: &Option<String>) -> Result<()> {
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

async fn handle_clone_session(ctx: &mut LiveCommandContext<'_, '_>) -> Result<()> {
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

async fn handle_resume_session<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    (session_id, io): (&str, &mut SessionCommandIo<'_, B>),
) -> Result<()> {
    *ctx.engine = crate::platform::agent_engine(
        ctx.session.config.clone(),
        ctx.session.auth_store.clone(),
        Some(session_id),
    )
    .await?;
    if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
        let _ = super::super::navigation::hydrate_session_transcript(io.controller, &tree, io.history);
    }
    ctx.session
        .renderer
        .print_status(&format!("Resumed session {session_id}"));
    Ok(())
}

async fn switch_session_result<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<()> {
    match result {
        CommandResult::ForkSession { turn_or_node_id } => handle_fork_session(ctx, turn_or_node_id).await?,
        CommandResult::CloneSession => handle_clone_session(ctx).await?,
        CommandResult::ResumeSession { session_id } => handle_resume_session(ctx, (session_id, io)).await?,
        _ => {}
    }
    Ok(())
}

async fn handle_switch_session_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<()> {
    if let CommandResult::SwitchBranch { leaf_id } = result {
        return switch_branch_for_command(ctx, io, leaf_id).await;
    }
    switch_session_result(ctx, io, result).await
}

async fn dispatch_session_result<B: TerminalBackend>(
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

async fn switch_branch_for_command<B: TerminalBackend>(
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

async fn handle_session_diagnostics<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: &mut SessionCommandIo<'_, B>,
    result: &CommandResult,
) -> Result<bool> {
    match result {
        CommandResult::OpenSessionSelector => {
            super::super::modal::open_session_selector(&ctx.session.config.sessions_dir, io.controller);
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

pub(super) async fn handle_session_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    mut io: SessionCommandIo<'_, B>,
    result: CommandResult,
) -> Result<bool> {
    dispatch_session_result(ctx, &mut io, &result).await
}

fn is_tree_arm(result: &CommandResult) -> bool {
    matches!(result, CommandResult::OpenTreeSelector | CommandResult::Tree)
}

fn is_session_switch_arm(result: &CommandResult) -> bool {
    matches!(
        result,
        CommandResult::SwitchBranch { .. }
            | CommandResult::ForkSession { .. }
            | CommandResult::CloneSession
            | CommandResult::ResumeSession { .. }
    )
}
