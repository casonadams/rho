use std::sync::Arc;

use rho_harness_core::config::Config;
use rig::completion::Usage;
use rig::memory::ConversationMemory;
use rig::message::Message;
use rig::test_utils::{MockCompletionModel, MockError, MockStreamEvent};

use super::common::CapturingPresenter;
use crate::engine::eval::mock::{MockEngineConfig, final_event, mock_engine};
use crate::engine::runner::TurnRequest;

#[tokio::test]
async fn test_context_overflow_auto_recovery_succeeds_on_retry() {
    let dir = std::env::temp_dir().join(format!("overflow_rec_{}", uuid::Uuid::new_v4()));
    let app_config = Config {
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
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::Error(MockError::provider(
            "context_length_exceeded: maximum context length is 128000 tokens",
        ))],
        vec![MockStreamEvent::text("recovered from overflow"), final_event(usage)],
    ]);

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
        vec![Message::user("Old turn 1"), Message::assistant("Old response 1")],
    )
    .await
    .unwrap();
    ConversationMemory::append(
        &engine.session_manager,
        &session_id,
        vec![Message::user("Old turn 2"), Message::assistant("Old response 2")],
    )
    .await
    .unwrap();

    let presenter = Arc::new(CapturingPresenter::default());
    let output = engine
        .run_turn(
            TurnRequest::new("New prompt that overflows initially"),
            presenter.clone(),
        )
        .await
        .unwrap();

    assert_eq!(output.final_text, "recovered from overflow");
    let notices = presenter.notices.lock().unwrap().clone();
    assert!(notices.iter().any(|n| n.contains("Context overflow detected")));
    assert!(notices.iter().any(|n| n.contains("Compacted context")));
}

#[tokio::test]
async fn test_context_overflow_fails_if_overflow_persists() {
    let dir = std::env::temp_dir().join(format!("overflow_loop_{}", uuid::Uuid::new_v4()));
    let app_config = Config {
        keep_recent_tokens: 5,
        auth_file: dir.join("auth.json"),
        ..Config::default()
    };
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::Error(MockError::provider("context_length_exceeded"))],
        vec![MockStreamEvent::Error(MockError::provider("context_length_exceeded"))],
    ]);

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
        vec![Message::user("Old turn 1"), Message::assistant("Old response 1")],
    )
    .await
    .unwrap();
    ConversationMemory::append(
        &engine.session_manager,
        &session_id,
        vec![Message::user("Old turn 2"), Message::assistant("Old response 2")],
    )
    .await
    .unwrap();

    let presenter = Arc::new(CapturingPresenter::default());
    let result = engine
        .run_turn(TurnRequest::new("Persistent overflow prompt"), presenter)
        .await;

    assert!(result.is_err());
}
