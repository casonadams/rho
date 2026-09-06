use rho_harness_core::config::Config;
use rho_harness_core::session::tree::TreeNodeKind;
use rig::agent::ModelHandle;
use rig::memory::ConversationMemory;
use rig::message::{
    AssistantContent, Message, Text, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};
use rig::test_utils::{MockCompletionModel, MockTurn};

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

async fn append_file_turns(sm: &rho_harness_core::session::SessionManager, sid: &str, turns: &[(&str, &str, &str)]) {
    for (call_id, tool, path) in turns {
        let turn = file_turn(call_id, tool, path);
        ConversationMemory::append(sm, sid, turn).await.unwrap();
    }
}

async fn assert_two_compactions(sm: &rho_harness_core::session::SessionManager) {
    let tree = sm.load_tree().await.unwrap();
    let leaf_id = tree.active_leaf_id.as_ref().unwrap();
    let count = tree
        .ancestor_nodes(leaf_id)
        .into_iter()
        .filter(|n| n.kind == TreeNodeKind::Compaction)
        .count();
    assert_eq!(count, 2);
}

fn assert_files_tracked(summary: &str) {
    for f in ["file1.txt", "file2.txt", "file3.txt", "file4.txt"] {
        assert!(summary.contains(f));
    }
}

fn assert_no_system_messages_in_prompts(requests: &[rig::completion::CompletionRequest]) {
    assert!(requests.len() >= 2);
    for req in &requests[1..] {
        assert!(!format!("{req:?}").contains("[System]: ## Goal"));
    }
}

fn sequential_mock() -> MockCompletionModel {
    MockCompletionModel::new([
        MockTurn::text("## Goal\nFirst compaction"),
        MockTurn::text("## Goal\nPrefix 1"),
        MockTurn::text("## Goal\nSecond compaction"),
        MockTurn::text("## Goal\nPrefix 2"),
    ])
}

#[tokio::test]
async fn test_sequential_compactions_accumulate_files() {
    let mock = sequential_mock();
    let engine = test_engine("seq", Some(mock.clone())).await;
    let sid = engine.session_manager.session_id.clone();

    append_file_turns(
        &engine.session_manager,
        &sid,
        &[("c1", "read", "file1.txt"), ("c2", "write", "file2.txt")],
    )
    .await;
    let s1 = engine.compact_session(None).await.unwrap();
    assert!(s1.summary.contains("file1.txt") && s1.summary.contains("file2.txt"));

    append_file_turns(
        &engine.session_manager,
        &sid,
        &[("c3", "edit", "file3.txt"), ("c4", "read", "file4.txt")],
    )
    .await;
    let s2 = engine.compact_session(None).await.unwrap();
    assert_files_tracked(&s2.summary);
    assert_two_compactions(&engine.session_manager).await;
    assert_no_system_messages_in_prompts(&mock.requests());
}
