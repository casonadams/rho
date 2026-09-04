use std::sync::Arc;

use rho_harness_core::config::Config;
use rho_harness_core::session::tree::TreeNodeKind;
use rig::completion::Usage;
use rig::memory::ConversationMemory;
use rig::message::Message;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::common::CapturingPresenter;
use crate::engine::eval::mock::{MockEngineConfig, final_event, mock_engine};
use crate::engine::runner::TurnRequest;

#[tokio::test]
async fn test_proactive_auto_compaction_before_turn() {
    let dir = std::env::temp_dir().join(format!("proactive_{}", uuid::Uuid::new_v4()));
    let app_config = Config {
        model: "mock-model".to_string(),
        reserve_tokens: 127_980,
        keep_recent_tokens: 5,
        auth_file: dir.join("auth.json"),
        ..Config::default()
    };
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
        ..Default::default()
    };
    let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::text("turn response"), final_event(usage)]]);
    let engine = mock_engine(
        model,
        MockEngineConfig {
            base_dir: &dir,
            app_config,
            session_manager: None,
            built_in_tools: None,
        },
    );

    let session_id = engine.session_manager.session_id.clone();
    ConversationMemory::append(
        &engine.session_manager,
        &session_id,
        vec![
            Message::user("Turn 1 request that will take several tokens in history"),
            Message::assistant("Turn 1 response that takes more tokens in context"),
        ],
    )
    .await
    .unwrap();
    ConversationMemory::append(
        &engine.session_manager,
        &session_id,
        vec![
            Message::user("Turn 2 request adding even more context"),
            Message::assistant("Turn 2 response completing second step"),
        ],
    )
    .await
    .unwrap();

    let presenter = Arc::new(CapturingPresenter::default());
    let output = engine
        .run_turn(TurnRequest::new("Turn 3 request"), presenter.clone())
        .await
        .unwrap();

    assert_eq!(output.final_text, "turn response");
    let notices = presenter.notices.lock().unwrap().clone();
    assert!(notices.iter().any(|n| n.contains("Auto-compacted context")));

    let tree = engine.session_manager.load_tree().await.unwrap();
    let leaf_id = tree.active_leaf_id.as_ref().unwrap();
    let nodes = tree.ancestor_nodes(leaf_id);
    assert!(nodes.iter().any(|n| n.kind == TreeNodeKind::Compaction));
}
