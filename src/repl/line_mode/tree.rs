use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;

pub async fn show_tree(session: &ReplSession, engine: &AgentEngine) -> Result<()> {
    let tree = engine.session_manager.load_tree().await?;
    let rendered = crate::ui::interactive::tree_view::render_tree_ascii(&tree);
    session.renderer.print_notice(&format!(
        "\nConversation Tree (Session: {}):\n{rendered}\n",
        engine.session_manager.session_id
    ));
    Ok(())
}

pub async fn switch_active_branch(leaf_id: String, session: &mut ReplSession, engine: &mut AgentEngine) -> Result<()> {
    let old_leaf = engine.session_manager.active_leaf_id().await?.unwrap_or_default();
    let tree = engine.session_manager.load_tree().await?;
    let (abandoned, _) = tree.branch_divergence(&old_leaf, &leaf_id);
    let has_assistant = abandoned
        .iter()
        .any(|n| n.kind == rho_harness_core::session::TreeNodeKind::AssistantTurn);
    if has_assistant
        && session.renderer.has_interactive_ui()
        && let Ok(true) = inquire::Confirm::new("Summarize discoveries from abandoned branch before switching?")
            .with_default(true)
            .prompt()
    {
        let summary_text = abandoned
            .iter()
            .map(|n| format!("{:?}", n.messages))
            .collect::<Vec<_>>()
            .join(" ");
        let _ = engine
            .session_manager
            .append_branch_summary(&summary_text, &old_leaf)
            .await;
    }
    let _ = engine.session_manager.switch_branch(Some(leaf_id.clone())).await?;
    *engine = engine
        .rebuild(session.config.clone(), session.auth_store.clone())
        .await?;
    session
        .renderer
        .print_notice(&format!("  [Switched active branch to {leaf_id}]\n"));
    Ok(())
}
