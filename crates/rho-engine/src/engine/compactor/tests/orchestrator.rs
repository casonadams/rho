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

#[tokio::test]
async fn test_compact_session_with_file_tracking_and_metrics() {
    let mock =
        MockCompletionModel::text("## Goal\nRefactor session storage\n\n## Progress\n### Done\n- [x] Read files");
    let engine = test_engine("file_tracking", Some(mock)).await;
    let session_id = engine.session_manager.session_id.clone();

    let turn1 = vec![
        Message::user("Please read the config and modify src/storage.rs"),
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("c1"),
                ToolFunction::new("read".to_string(), serde_json::json!({"path": "Cargo.toml"})),
            ))],
        },
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("c1"),
                provider: None,
                name: "read".to_string(),
                content: vec![ToolResultContent::Text(Text::new("[package]\nname = \"rho\""))],
            })],
        },
        Message::assistant("I read Cargo.toml."),
    ];

    let turn2 = vec![
        Message::user("Now edit src/storage.rs"),
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("c2"),
                ToolFunction::new(
                    "write".to_string(),
                    serde_json::json!({"path": "src/storage.rs", "content": "pub fn init() {}"}),
                ),
            ))],
        },
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("c2"),
                provider: None,
                name: "write".to_string(),
                content: vec![ToolResultContent::Text(Text::new("Wrote 20 bytes"))],
            })],
        },
        Message::assistant("Wrote src/storage.rs."),
    ];

    let turn3 = vec![
        Message::user("Final step: verify everything"),
        Message::assistant("All verified."),
    ];

    ConversationMemory::append(&engine.session_manager, &session_id, turn1)
        .await
        .unwrap();
    ConversationMemory::append(&engine.session_manager, &session_id, turn2)
        .await
        .unwrap();
    ConversationMemory::append(&engine.session_manager, &session_id, turn3)
        .await
        .unwrap();

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
