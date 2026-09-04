use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub(super) struct BranchSwitchContext<'a, 'b, 'c, B: TerminalBackend> {
    pub session: &'a mut ReplSession,
    pub engine: &'b mut AgentEngine,
    pub controller: &'c mut TerminalController<B>,
    pub history: &'c mut InteractiveHistory,
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
    if has_assistant
        && ctx.session.renderer.has_interactive_ui()
        && let Ok(true) = inquire::Confirm::new("Summarize discoveries from abandoned branch before switching?")
            .with_default(true)
            .prompt()
    {
        let summary_text = abandoned
            .iter()
            .map(|n| format!("{:?}", n.messages))
            .collect::<Vec<_>>()
            .join(" ");
        let _ = ctx
            .engine
            .session_manager
            .append_branch_summary(&summary_text, &old_leaf)
            .await;
    }
    ctx.engine.session_manager.switch_branch(Some(leaf_id.clone())).await?;
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
