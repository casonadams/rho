use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::input_reader::TerminalInputReader;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub(super) struct BranchSwitchContext<'a, 'b, 'c, B: TerminalBackend> {
    pub session: &'a mut ReplSession,
    pub engine: &'b mut AgentEngine,
    pub controller: &'c mut TerminalController<B>,
    pub history: &'c mut InteractiveHistory,
    pub input: &'c mut TerminalInputReader,
}

pub(super) async fn handle_switch_branch<B: TerminalBackend>(
    ctx: BranchSwitchContext<'_, '_, '_, B>,
    leaf_id: String,
) -> Result<()> {
    let old_leaf = ctx.engine.session_manager.active_leaf_id().await?.unwrap_or_default();
    let tree = ctx.engine.session_manager.load_tree().await?;
    let (abandoned, _) = tree.branch_divergence(&old_leaf, &leaf_id);
    let has_assistant = abandoned
        .iter()
        .any(|n| n.kind == rho_harness_core::session::TreeNodeKind::AssistantTurn);
    let summary_text = if has_assistant && ctx.session.renderer.has_interactive_ui() {
        let mut paused = ctx.input.pause()?;
        paused.drain();
        ctx.controller.suspend()?;
        let confirmed = inquire::Confirm::new("Summarize discoveries from abandoned branch before switching?")
            .with_default(true)
            .prompt();
        let controller_res = ctx.controller.resume();
        let input_res = paused.resume();
        ctx.input.drain();
        controller_res?;
        input_res?;
        if let Ok(true) = confirmed {
            let messages: Vec<_> = abandoned.iter().flat_map(|n| n.messages.clone()).collect();
            Some(ctx.engine.summarize_branch(&messages).await)
        } else {
            None
        }
    } else {
        None
    };
    ctx.engine.session_manager.switch_branch(Some(leaf_id.clone())).await?;
    if let Some(summary) = summary_text {
        let _ = ctx
            .engine
            .session_manager
            .append_branch_summary(&summary, &old_leaf)
            .await;
    }
    *ctx.engine = ctx
        .engine
        .rebuild(ctx.session.config.clone(), ctx.session.auth_store.clone())
        .await?;
    if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
        let _ = super::super::navigation::hydrate_session_transcript(ctx.controller, &tree, ctx.history);
    }
    ctx.session
        .renderer
        .print_status(&format!("Switched active branch to {leaf_id}"));
    Ok(())
}
