use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::input_reader::TerminalInputReader;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{TerminalBackend, TerminalController};
use rho_harness_core::session::TreeNodeKind;

pub(super) struct BranchSwitchContext<'a, 'b, 'c, B: TerminalBackend> {
    pub session: &'a mut ReplSession,
    pub engine: &'b mut AgentEngine,
    pub controller: &'c mut TerminalController<B>,
    pub history: &'c mut InteractiveHistory,
    pub input: &'c mut TerminalInputReader,
}

fn confirm_branch_summary() -> bool {
    inquire::Confirm::new("Summarize discoveries from abandoned branch before switching?")
        .with_default(true)
        .prompt()
        .is_ok_and(|v| v)
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

async fn append_branch_summary(
    ctx: &mut BranchSwitchContext<'_, '_, '_, impl TerminalBackend>,
    (summary, old_leaf): (&str, &str),
) {
    let _ = ctx
        .engine
        .session_manager
        .append_branch_summary(summary, old_leaf)
        .await;
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
        let _ = super::super::navigation::hydrate_session_transcript(ctx.controller, &tree, ctx.history);
    }
    ctx.session
        .renderer
        .print_status(&format!("Switched active branch to {leaf_id}"));
    Ok(())
}

async fn switch_and_hydrate(
    ctx: &mut BranchSwitchContext<'_, '_, '_, impl TerminalBackend>,
    (leaf_id, summary, old_leaf): (&str, Option<String>, &str),
) -> Result<()> {
    ctx.engine
        .session_manager
        .switch_branch(Some(leaf_id.to_string()))
        .await?;
    if let Some(summary) = summary.as_deref() {
        append_branch_summary(ctx, (summary, old_leaf)).await;
    }
    rebuild_and_hydrate(ctx, leaf_id).await
}

pub(super) async fn handle_switch_branch<B: TerminalBackend>(
    mut ctx: BranchSwitchContext<'_, '_, '_, B>,
    leaf_id: String,
) -> Result<()> {
    let old_leaf = ctx.engine.session_manager.active_leaf_id().await?.unwrap_or_default();
    let tree = ctx.engine.session_manager.load_tree().await?;
    let (abandoned, _) = tree.branch_divergence(&old_leaf, &leaf_id);
    let summary = maybe_summarize_abandoned(&mut ctx, &abandoned).await;
    switch_and_hydrate(&mut ctx, (&leaf_id, summary, &old_leaf)).await
}
