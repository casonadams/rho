use super::super::helpers::{final_event, presenter, request, test_engine, test_engine_with_session};
use crate::config::Config;
use crate::session::SessionManager;
use crate::ui::TerminalRenderer;
use rig::completion::Usage;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use std::path::PathBuf;

fn budget_exhausted_model() -> MockCompletionModel {
    MockCompletionModel::from_stream_turns([
        [
            MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path":"missing-a"})),
            final_event(Usage::new()),
        ],
        [
            MockStreamEvent::tool_call("call-2", "read", serde_json::json!({"path":"missing-b"})),
            final_event(Usage::new()),
        ],
    ])
}

async fn setup_budget_exhausted_checkpoint() -> (String, PathBuf) {
    let first = test_engine(
        budget_exhausted_model(),
        Config {
            max_turns: 2,
            ..Config::default()
        },
    );
    let _ = first
        .run_turn(
            request("inspect the repository"),
            presenter(&TerminalRenderer::default()),
        )
        .await;
    let id = first.session_manager.session_id.clone();
    let dir = first.session_manager.file_path.parent().unwrap().to_path_buf();
    (id, dir)
}

fn assert_resumed_history_promoted(history: &[rig::message::Message]) {
    let encoded = serde_json::to_string(history).unwrap();
    assert_eq!(encoded.matches("inspect the repository").count(), 1);
    assert_eq!(
        (
            encoded.matches("missing-a").count(),
            encoded.matches("missing-b").count()
        ),
        (2, 2)
    );
}

async fn assert_sm_checkpoint_empty(sm: &SessionManager) {
    assert!(sm.load_checkpoint().await.unwrap().is_none());
    assert_eq!(sm.load_messages().await.unwrap().len(), 7);
}

async fn assert_checkpoint_promoted_in_stores(
    engine: &crate::engine::AgentEngine,
    (dir, id): (&std::path::Path, &str),
) {
    assert_sm_checkpoint_empty(&engine.session_manager).await;
    assert_sm_checkpoint_empty(&SessionManager::new(dir, Some(id)).unwrap()).await;
}

#[tokio::test]
async fn budget_exhausted_checkpoint_survives_process_resume_and_promotes_once() {
    let (id, dir) = setup_budget_exhausted_checkpoint().await;
    let resumed_store = SessionManager::new(&dir, Some(&id)).unwrap();
    let resumed_model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::text("repository summary"),
        final_event(Usage::new()),
    ]]);
    let resumed = test_engine_with_session(
        resumed_model.clone(),
        Config {
            max_turns: 2,
            ..Config::default()
        },
        Some(resumed_store),
    );
    resumed
        .run_turn(request("please continue"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    assert_resumed_history_promoted(&resumed_model.requests()[0].chat_history);
    assert_checkpoint_promoted_in_stores(&resumed, (&dir, &id)).await;
}

async fn setup_single_turn_checkpoint(probe: &str) -> (Vec<rig::message::Message>, String, PathBuf) {
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path": probe})),
        final_event(Usage::new()),
    ]]);
    let first = test_engine(
        model,
        Config {
            max_turns: 1,
            ..Config::default()
        },
    );
    first
        .run_turn(request("inspect"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap_err();
    let cp = first.session_manager.load_checkpoint().await.unwrap().unwrap();
    let id = first.session_manager.session_id.clone();
    let dir = first.session_manager.file_path.parent().unwrap().to_path_buf();
    (cp, id, dir)
}

fn assert_requests_contain_probe(requests: &[rig::completion::CompletionRequest], probe: &str) {
    for req in requests {
        let history = serde_json::to_string(&req.chat_history).unwrap();
        assert_eq!(history.matches(probe).count(), 2);
    }
}

async fn assert_retry_session_state(engine: &crate::engine::AgentEngine, (dir, id): (&std::path::Path, &str)) {
    assert!(engine.session_manager.load_checkpoint().await.unwrap().is_none());
    assert_eq!(
        SessionManager::new(dir, Some(id))
            .unwrap()
            .load_messages()
            .await
            .unwrap()
            .len(),
        5
    );
}

#[tokio::test]
async fn failed_checkpoint_continuation_remains_available_until_success() {
    let (checkpoint, id, dir) = setup_single_turn_checkpoint("checkpoint-probe-missing-3f9b").await;
    let resumed_store = SessionManager::new(&dir, Some(&id)).unwrap();
    let resumed_model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("offline provider failure")],
        vec![MockStreamEvent::text("done"), final_event(Usage::new())],
    ]);
    let resumed = test_engine_with_session(resumed_model.clone(), Config::default(), Some(resumed_store));
    resumed
        .run_turn(request("continue"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap_err();
    assert_eq!(
        resumed.session_manager.load_checkpoint().await.unwrap(),
        Some(checkpoint)
    );

    resumed
        .run_turn(request("continue again"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();
    assert_requests_contain_probe(&resumed_model.requests(), "checkpoint-probe-missing-3f9b");
    assert_retry_session_state(&resumed, (&dir, &id)).await;
}
