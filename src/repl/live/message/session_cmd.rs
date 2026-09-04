use super::branch::{BranchSwitchContext, handle_switch_branch};
use super::command::LiveCommandContext;
use crate::error::Result;
use crate::repl::commands::CommandResult;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub(super) struct SessionCommandIo<'a, B: TerminalBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub history: &'a mut InteractiveHistory,
}

pub(super) async fn handle_session_command<B: TerminalBackend>(
    ctx: &mut LiveCommandContext<'_, '_>,
    io: SessionCommandIo<'_, B>,
    result: CommandResult,
) -> Result<bool> {
    match result {
        CommandResult::OpenTreeSelector => {
            let tree = ctx.engine.session_manager.load_tree().await?;
            super::super::modal::open_tree_selector(&tree, io.controller);
            io.controller.redraw()?;
        }
        CommandResult::Tree => {
            let tree = ctx.engine.session_manager.load_tree().await?;
            let rendered = crate::ui::interactive::tree_view::render_tree_ascii(&tree);
            ctx.session.renderer.print_notice(&format!(
                "\nConversation Tree (Session: {}):\n{rendered}\n",
                ctx.engine.session_manager.session_id
            ));
        }
        CommandResult::SwitchBranch { leaf_id } => {
            handle_switch_branch(
                BranchSwitchContext {
                    session: ctx.session,
                    engine: ctx.engine,
                    controller: io.controller,
                    history: io.history,
                },
                leaf_id,
            )
            .await?;
        }
        CommandResult::ForkSession { turn_or_node_id } => {
            let forked = ctx
                .engine
                .session_manager
                .fork_session(&ctx.session.config.sessions_dir, turn_or_node_id.as_deref())
                .await?;
            ctx.session
                .renderer
                .print_status(&format!("Forked session: {}", forked.session_id));
        }
        CommandResult::CloneSession => {
            let cloned = ctx
                .engine
                .session_manager
                .clone_session(&ctx.session.config.sessions_dir)
                .await?;
            ctx.session
                .renderer
                .print_status(&format!("Cloned session: {}", cloned.session_id));
        }
        CommandResult::OpenSessionSelector => {
            super::super::modal::open_session_selector(&ctx.session.config.sessions_dir, io.controller);
            io.controller.redraw()?;
        }
        CommandResult::ResumeSession { session_id } => {
            *ctx.engine = crate::platform::agent_engine(
                ctx.session.config.clone(),
                ctx.session.auth_store.clone(),
                Some(&session_id),
            )
            .await?;
            if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
                let _ = super::super::navigation::hydrate_session_transcript(io.controller, &tree, io.history);
            }
            ctx.session
                .renderer
                .print_status(&format!("Resumed session {session_id}"));
        }
        CommandResult::NameSession { name } => {
            ctx.engine.session_manager.set_session_name(&name).await?;
            ctx.session.renderer.print_status(&format!("Session name: \"{name}\""));
        }
        CommandResult::Rewind { turn } => {
            let retained_count = ctx.engine.session_manager.rewind_to_turn(turn).await?;
            ctx.session.renderer.print_notice(&format!(
                "  [Rewound context to Turn {turn} ({retained_count} messages retained)]\n"
            ));
        }
        _ => return Ok(false),
    }
    Ok(true)
}
