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

async fn maybe_summarize_abandoned(
    abandoned: &[&rho_harness_core::session::tree::TreeNodeData],
    engine: &AgentEngine,
    has_ui: bool,
) -> Option<String> {
    let has_assistant = abandoned
        .iter()
        .any(|n| n.kind == rho_harness_core::session::TreeNodeKind::AssistantTurn);
    if !has_assistant || !has_ui {
        return None;
    }
    let confirm = inquire::Confirm::new("Summarize discoveries from abandoned branch before switching?")
        .with_default(true)
        .prompt()
        .ok()?;
    if !confirm {
        return None;
    }
    let messages: Vec<_> = abandoned.iter().flat_map(|n| n.messages.clone()).collect();
    Some(engine.summarize_branch(&messages).await)
}

async fn apply_branch_switch(
    leaf_id: &str,
    (summary, old_leaf): (Option<&str>, &str),
    engine: &mut AgentEngine,
) -> Result<()> {
    let _ = engine.session_manager.switch_branch(Some(leaf_id.to_string())).await?;
    if let Some(s) = summary {
        let _ = engine.session_manager.append_branch_summary(s, old_leaf).await;
    }
    Ok(())
}

pub async fn switch_active_branch(leaf_id: String, session: &mut ReplSession, engine: &mut AgentEngine) -> Result<()> {
    let (old_leaf, tree) = load_branch_context(engine).await?;
    let (abandoned, _) = tree.branch_divergence(&old_leaf, &leaf_id);
    let summary = maybe_summarize_abandoned(&abandoned, engine, session.renderer.has_interactive_ui()).await;

    apply_branch_switch(&leaf_id, (summary.as_deref(), &old_leaf), engine).await?;
    *engine = engine
        .rebuild(session.config.clone(), session.auth_store.clone())
        .await?;
    session
        .renderer
        .print_notice(&format!("  [Switched active branch to {leaf_id}]\n"));
    Ok(())
}

async fn load_branch_context(engine: &AgentEngine) -> Result<(String, rho_harness_core::session::tree::SessionTree)> {
    let old_leaf = engine.session_manager.active_leaf_id().await?.unwrap_or_default();
    let tree = engine.session_manager.load_tree().await?;
    Ok((old_leaf, tree))
}
