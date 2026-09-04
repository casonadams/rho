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
    let dir = std::env::temp_dir().join(format!("sequential_{label}_{}", uuid::Uuid::new_v4()));
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

fn file_turn(call_id: &str, tool: &str, path: &str) -> Vec<Message> {
    vec![
        Message::user(format!("Execute {tool} on {path}")),
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint(call_id),
                ToolFunction::new(tool.to_string(), serde_json::json!({"path": path})),
            ))],
        },
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint(call_id),
                provider: None,
                name: tool.to_string(),
                content: vec![ToolResultContent::Text(Text::new("done"))],
            })],
        },
        Message::assistant(format!("Completed {tool} on {path}.")),
    ]
}

#[tokio::test]
async fn test_sequential_compactions_accumulate_files() {
    let mock = MockCompletionModel::text("## Goal\nProgress summary");
    let engine = test_engine("seq", Some(mock)).await;
    let session_id = engine.session_manager.session_id.clone();

    ConversationMemory::append(
        &engine.session_manager,
        &session_id,
        file_turn("c1", "read", "file1.txt"),
    )
    .await
    .unwrap();
    ConversationMemory::append(
        &engine.session_manager,
        &session_id,
        file_turn("c2", "write", "file2.txt"),
    )
    .await
    .unwrap();

    let stats1 = engine.compact_session(None).await.unwrap();
    assert!(stats1.summary.contains("file1.txt"));
    assert!(stats1.summary.contains("file2.txt"));

    ConversationMemory::append(
        &engine.session_manager,
        &session_id,
        file_turn("c3", "edit", "file3.txt"),
    )
    .await
    .unwrap();
    ConversationMemory::append(
        &engine.session_manager,
        &session_id,
        file_turn("c4", "read", "file4.txt"),
    )
    .await
    .unwrap();

    let stats2 = engine.compact_session(None).await.unwrap();
    assert!(stats2.summary.contains("file1.txt"));
    assert!(stats2.summary.contains("file2.txt"));
    assert!(stats2.summary.contains("file3.txt"));
    assert!(stats2.summary.contains("file4.txt"));

    let tree = engine.session_manager.load_tree().await.unwrap();
    let leaf_id = tree.active_leaf_id.as_ref().unwrap();
    let nodes = tree.ancestor_nodes(leaf_id);
    let compactions: Vec<_> = nodes.iter().filter(|n| n.kind == TreeNodeKind::Compaction).collect();
    assert_eq!(compactions.len(), 2);
}
