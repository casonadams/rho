use std::sync::Arc;

use rho_harness_core::config::Config;
use rig::completion::Usage;
use rig::memory::ConversationMemory;
use rig::message::Message;
use rig::test_utils::{MockCompletionModel, MockError, MockStreamEvent};

use super::common::CapturingPresenter;
use crate::engine::eval::mock::{MockEngineConfig, final_event, mock_engine};
use crate::engine::runner::TurnRequest;

async fn populate_recovery_history(sm: &rho_harness_core::session::SessionManager, sid: &str) {
    for i in 1..=2 {
        let msgs = vec![
            Message::user(format!("Old turn {i}")),
            Message::assistant(format!("Old response {i}")),
        ];
        ConversationMemory::append(sm, sid, msgs).await.unwrap();
    }
}

fn assert_recovery_presenter(presenter: &CapturingPresenter) {
    let notices = presenter.notices.lock().unwrap().clone();
    assert!(notices.iter().any(|n| n.contains("Context overflow detected")));
    assert!(notices.iter().any(|n| n.contains("Compacted context")));
    assert!(
        presenter
            .spinner_messages
            .lock()
            .unwrap()
            .iter()
            .any(|m| m == "Compacting...")
    );
}

fn recovery_engine(dir: &std::path::Path, model: MockCompletionModel) -> crate::engine::AgentEngine {
    let app_config = Config {
        keep_recent_tokens: 5,
        auth_file: dir.join("auth.json"),
        ..Config::default()
    };
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

fn sample_overflow_usage() -> Usage {
    Usage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
        ..Default::default()
    }
}

#[tokio::test]
async fn test_context_overflow_auto_recovery_succeeds_on_retry() {
    let dir = std::env::temp_dir().join(format!("overflow_rec_{}", uuid::Uuid::new_v4()));
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::Error(MockError::provider(
            "context_length_exceeded: maximum context length is 128000 tokens",
        ))],
        vec![
            MockStreamEvent::text("recovered from overflow"),
            final_event(sample_overflow_usage()),
        ],
    ]);

    let engine = recovery_engine(&dir, model);
    let session_id = engine.session_manager.session_id.clone();
    populate_recovery_history(&engine.session_manager, &session_id).await;

    let presenter = Arc::new(CapturingPresenter::default());
    let output = engine
        .run_turn(
            TurnRequest::new("New prompt that overflows initially"),
            presenter.clone(),
        )
        .await
        .unwrap();
    assert_eq!(output.final_text, "recovered from overflow");
    assert_recovery_presenter(&presenter);
}

#[tokio::test]
async fn test_context_overflow_fails_if_overflow_persists() {
    let dir = std::env::temp_dir().join(format!("overflow_loop_{}", uuid::Uuid::new_v4()));
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::Error(MockError::provider("context_length_exceeded"))],
        vec![MockStreamEvent::Error(MockError::provider("context_length_exceeded"))],
    ]);

    let engine = recovery_engine(&dir, model);
    let session_id = engine.session_manager.session_id.clone();
    populate_recovery_history(&engine.session_manager, &session_id).await;

    let presenter = Arc::new(CapturingPresenter::default());
    let result = engine
        .run_turn(TurnRequest::new("Persistent overflow prompt"), presenter)
        .await;
    assert!(result.is_err());
}
