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

async fn populate_proactive_history(sm: &rho_harness_core::session::SessionManager, sid: &str) {
    for i in 1..=2 {
        let msgs = vec![
            Message::user(format!("Turn {i} request with many tokens")),
            Message::assistant(format!("Turn {i} response")),
        ];
        ConversationMemory::append(sm, sid, msgs).await.unwrap();
    }
}

fn assert_compaction_presenter(presenter: &CapturingPresenter) {
    let notices = presenter.notices.lock().unwrap().clone();
    assert!(notices.iter().any(|n| n.contains("Auto-compacted context")));
    assert!(
        presenter
            .spinner_messages
            .lock()
            .unwrap()
            .iter()
            .any(|m| m == "Compacting...")
    );
}

fn proactive_engine(dir: &std::path::Path, reserve_tokens: usize) -> crate::engine::AgentEngine {
    let app_config = Config {
        model: "mock-model".to_string(),
        reserve_tokens,
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
    mock_engine(
        model,
        MockEngineConfig {
            base_dir: dir,
            app_config,
            session_manager: None,
            built_in_tools: None,
        },
    )
}

async fn assert_compaction_node_present(sm: &rho_harness_core::session::SessionManager) {
    let tree = sm.load_tree().await.unwrap();
    let leaf_id = tree.active_leaf_id.as_ref().unwrap();
    assert!(
        tree.ancestor_nodes(leaf_id)
            .iter()
            .any(|n| n.kind == TreeNodeKind::Compaction)
    );
}

#[tokio::test]
async fn test_proactive_auto_compaction_before_turn() {
    let dir = std::env::temp_dir().join(format!("proactive_{}", uuid::Uuid::new_v4()));
    let engine = proactive_engine(&dir, 127_980);
    let session_id = engine.session_manager.session_id.clone();
    populate_proactive_history(&engine.session_manager, &session_id).await;

    let presenter = Arc::new(CapturingPresenter::default());
    let output = engine
        .run_turn(TurnRequest::new("Turn 3 request"), presenter.clone())
        .await
        .unwrap();
    assert_eq!(output.final_text, "turn response");
    assert_compaction_presenter(&presenter);
    assert_compaction_node_present(&engine.session_manager).await;
}

fn record_test_turn_usage(engine: &crate::engine::AgentEngine, tokens: u64) {
    let u = Usage {
        input_tokens: tokens,
        ..Default::default()
    }
    .into();
    engine
        .usage
        .record_turn(crate::engine::tracking::TurnUsage::new(u, u), 100);
}

async fn check_compaction_step(
    engine: &crate::engine::AgentEngine,
    presenter: &CapturingPresenter,
    (history, tokens): (&mut Vec<Message>, u64),
) {
    record_test_turn_usage(engine, tokens);
    engine
        .check_proactive_compaction(presenter, (history, 0))
        .await
        .unwrap();
}

fn threshold_engine(dir: &std::path::Path) -> crate::engine::AgentEngine {
    let app_config = Config {
        model: "mock-model".to_string(),
        keep_recent_tokens: 5,
        auth_file: dir.join("auth.json"),
        ..Config::default()
    };
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("response"), final_event(Usage::default())]]);
    mock_engine(
        model,
        MockEngineConfig {
            base_dir: dir,
            app_config,
            session_manager: None,
            built_in_tools: None,
        },
    )
}

async fn seed_threshold_history(engine: &crate::engine::AgentEngine) -> Vec<Message> {
    let sid = &engine.session_manager.session_id;
    let msgs = vec![Message::user("prior prompt"), Message::assistant("prior response")];
    ConversationMemory::append(&engine.session_manager, sid, msgs)
        .await
        .unwrap();
    ConversationMemory::load(&engine.session_manager, sid).await.unwrap()
}

#[tokio::test]
async fn test_proactive_auto_compaction_at_96_percent_threshold() {
    let dir = std::env::temp_dir().join(format!("proactive_96_{}", uuid::Uuid::new_v4()));
    let engine = threshold_engine(&dir);
    let mut history = seed_threshold_history(&engine).await;
    let presenter = Arc::new(CapturingPresenter::default());

    check_compaction_step(&engine, &presenter, (&mut history, 122_000)).await;
    assert!(presenter.notices.lock().unwrap().is_empty());
    check_compaction_step(&engine, &presenter, (&mut history, 122_880)).await;
    let notices = presenter.notices.lock().unwrap().clone();
    assert!(notices.iter().any(|n| n.contains("Auto-compacted context")));
}
