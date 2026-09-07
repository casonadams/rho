use rho_harness_core::config::Config;
use rho_harness_core::session::tree::TreeNodeKind;
use rig::agent::ModelHandle;
use rig::memory::ConversationMemory;
use rig::message::{
    AssistantContent, Message, Text, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};
use rig::test_utils::MockCompletionModel;

use crate::auth::AuthStore;
use crate::engine::AgentEngine;
use crate::engine::builder::AgentEngineBuilder;

async fn test_engine(label: &str, model: Option<MockCompletionModel>) -> AgentEngine {
    let dir = std::env::temp_dir().join(format!("orchestrator_{label}_{}", uuid::Uuid::new_v4()));
    let config = Config {
        sessions_dir: dir.join("sessions"),
        auth_file: dir.join("auth.json"),
        keep_recent_tokens: 10,
        ..Default::default()
    };
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let mut builder = AgentEngineBuilder::new(config, auth_store)
        .base_dir(dir)
        .tools(Vec::new());
    if let Some(m) = model {
        builder = builder.model(ModelHandle::new(m));
    }
    builder.build().await.unwrap()
}

fn tool_turn(cid: &str, tool: &str, path: &str, user: &str, done: &str) -> Vec<Message> {
    let call = ToolCall::new(
        ToolCallId::new_or_mint(cid),
        ToolFunction::new(tool.to_string(), serde_json::json!({"path": path})),
    );
    let res = ToolResult {
        call: ToolCallId::new_or_mint(cid),
        provider: None,
        name: tool.to_string(),
        content: vec![ToolResultContent::Text(Text::new("data"))],
    };
    vec![
        Message::user(user),
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        Message::User {
            content: vec![UserContent::ToolResult(res)],
        },
        Message::assistant(done),
    ]
}

async fn populate_test_turns(sm: &rho_harness_core::session::SessionManager, sid: &str) {
    let turn1 = tool_turn("c1", "read", "Cargo.toml", "Read config", "Done read");
    let turn2 = tool_turn("c2", "write", "src/storage.rs", "Edit storage", "Done write");
    for turn in [
        turn1,
        turn2,
        vec![Message::user("Verify"), Message::assistant("Verified")],
    ] {
        ConversationMemory::append(sm, sid, turn).await.unwrap();
    }
}

#[tokio::test]
async fn test_compact_session_with_file_tracking_and_metrics() {
    let mock =
        MockCompletionModel::text("## Goal\nRefactor session storage\n\n## Progress\n### Done\n- [x] Read files");
    let engine = test_engine("file_tracking", Some(mock)).await;
    let session_id = engine.session_manager.session_id.clone();
    populate_test_turns(&engine.session_manager, &session_id).await;

    let stats = engine.compact_session(Some("Focus on storage refactor")).await.unwrap();
    assert!(stats.tokens_before > 0);
    assert!(stats.tokens_after > 0);
    assert!(stats.summary.contains("Refactor session storage"));
    assert!(stats.summary.contains("<read-files>"));
    assert!(stats.summary.contains("Cargo.toml"));
    assert!(stats.summary.contains("<modified-files>"));
    assert!(stats.summary.contains("src/storage.rs"));

    let tree = engine.session_manager.load_tree().await.unwrap();
    let leaf_id = tree.active_leaf_id.as_ref().unwrap();
    let nodes = tree.ancestor_nodes(leaf_id);
    let comp_node = nodes.iter().find(|n| n.kind == TreeNodeKind::Compaction);
    assert!(comp_node.is_some());

    let meta = comp_node.unwrap().compaction_metadata().unwrap();
    assert_eq!(meta.custom_instructions.as_deref(), Some("Focus on storage refactor"));
    assert!(meta.read_files.contains(&"Cargo.toml".to_string()));
    assert!(meta.modified_files.contains(&"src/storage.rs".to_string()));

    let active_messages = tree.active_messages();
    assert!(matches!(&active_messages[0], Message::System { .. }));
}

#[tokio::test]
async fn test_compact_session_empty_or_single_node() {
    let engine = test_engine("empty_session", None).await;
    let stats = engine.compact_session(None).await.unwrap();
    assert_eq!(stats.saved_tokens, 0);

    let session_id = engine.session_manager.session_id.clone();
    ConversationMemory::append(
        &engine.session_manager,
        &session_id,
        vec![Message::user("Hello"), Message::assistant("Hi")],
    )
    .await
    .unwrap();

    let stats2 = engine.compact_session(None).await.unwrap();
    assert_eq!(stats2.saved_tokens, 0);
}

fn split_turn_fixture() -> Vec<Message> {
    vec![
        Message::user("Preamble prompt"),
        Message::assistant("Early reply"),
        Message::user("Tool result 1"),
        Message::assistant("Mid reply with many tokens to force cut point selection"),
        Message::user("Tool result 2"),
        Message::assistant("Final suffix reply"),
    ]
}

fn assert_preamble_omitted(messages: &[Message]) {
    assert!(matches!(&messages[0], Message::System { .. }));
    assert!(!messages.iter().any(|m| match m {
        Message::User { content } => content.iter().any(|c| match c {
            rig::message::UserContent::Text(t) => t.text.contains("Preamble prompt"),
            _ => false,
        }),
        _ => false,
    }));
}

#[tokio::test]
async fn test_compact_session_split_turn_prunes_prefix_messages() {
    let mock = MockCompletionModel::text("## Goal\nComplete huge operation");
    let engine = test_engine("split_turn", Some(mock)).await;
    let sid = engine.session_manager.session_id.clone();
    ConversationMemory::append(&engine.session_manager, &sid, split_turn_fixture())
        .await
        .unwrap();

    let stats = engine.compact_session(None).await.unwrap();
    assert!(stats.tokens_before > 0);

    let tree = engine.session_manager.load_tree().await.unwrap();
    let leaf_id = tree.active_leaf_id.as_ref().unwrap();
    let comp_node = tree
        .ancestor_nodes(leaf_id)
        .into_iter()
        .find(|n| n.kind == TreeNodeKind::Compaction)
        .unwrap();
    assert!(comp_node.compaction_metadata().unwrap().first_kept_node_id.is_some());
    assert_preamble_omitted(&tree.active_messages());
}
