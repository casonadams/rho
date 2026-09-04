use super::super::helpers::{final_event, presenter, request, test_engine, test_engine_with_session};
use crate::auth::AuthStore;
use crate::config::Config;
use crate::session::SessionManager;
use crate::ui::TerminalRenderer;
use rig::completion::Usage;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

#[tokio::test]
async fn two_prompts_receive_prior_canonical_history_exactly_once() {
    let model = MockCompletionModel::from_stream_turns([
        [MockStreamEvent::text("first answer"), final_event(Usage::new())],
        [MockStreamEvent::text("second answer"), final_event(Usage::new())],
    ]);
    let engine = test_engine(model.clone(), Config::default());
    engine
        .run_turn(request("first prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();
    engine
        .run_turn(request("second prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    let second = &model.requests()[1].chat_history;
    let encoded = serde_json::to_string(second).unwrap();
    assert_eq!(second.len(), 4, "{encoded}");
    assert_eq!(encoded.matches("first prompt").count(), 1);
    assert_eq!(encoded.matches("first answer").count(), 1);
    assert_eq!(encoded.matches("second prompt").count(), 1);
}

#[tokio::test]
async fn process_style_reopen_resumes_canonical_history_once() {
    let first_model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::text("persisted answer"),
        final_event(Usage::new()),
    ]]);
    let first = test_engine(first_model, Config::default());
    first
        .run_turn(request("persisted prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();
    let id = first.session_manager.session_id.clone();
    let dir = first.session_manager.file_path.parent().unwrap().to_path_buf();
    drop(first);

    let resumed_store = SessionManager::new(&dir, Some(&id)).unwrap();
    let resumed_model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("resumed answer"), final_event(Usage::new())]]);
    let resumed = test_engine_with_session(resumed_model.clone(), Config::default(), Some(resumed_store));
    resumed
        .run_turn(request("resume prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    let history = &resumed_model.requests()[0].chat_history;
    let encoded = serde_json::to_string(history).unwrap();
    assert_eq!(history.len(), 4, "{encoded}");
    assert_eq!(encoded.matches("persisted prompt").count(), 1);
    assert_eq!(encoded.matches("persisted answer").count(), 1);
}

#[tokio::test]
async fn model_rebuild_preserves_compatible_history_without_duplication() {
    let config = Config {
        provider: "ollama".to_string(),
        model: "first-local-model".to_string(),
        ..Config::default()
    };
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::text("stored answer"), final_event(Usage::new())]]);
    let engine = test_engine(model, config.clone());
    engine
        .run_turn(request("stored prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();
    let id = engine.session_manager.session_id.clone();
    let rebuilt = engine
        .rebuild(
            Config {
                model: "second-local-model".to_string(),
                ..config
            },
            AuthStore::default(),
        )
        .await
        .unwrap();

    assert_eq!(rebuilt.session_manager.session_id, id);
    let encoded = serde_json::to_string(&rebuilt.session_manager.load_messages().await.unwrap()).unwrap();
    assert_eq!(encoded.matches("stored prompt").count(), 1);
    assert_eq!(encoded.matches("stored answer").count(), 1);
}
