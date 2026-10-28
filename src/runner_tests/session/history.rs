use super::super::helpers::{presenter, request, test_engine, test_engine_with_session};
use crate::auth::AuthStore;
use crate::config::Config;
use crate::session::SessionManager;
use crate::ui::TerminalRenderer;
use rho_engine::provider::adapter::{MockAdapter, ModelStreamEvent};
use std::sync::Arc;

#[tokio::test]
async fn two_prompts_receive_prior_canonical_history_exactly_once() {
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::Text("first answer".to_string()))]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text("second answer".to_string()))]);
    let engine = test_engine(model.clone(), Config::default());
    engine
        .run_turn(request("first prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();
    engine
        .run_turn(request("second prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    let second = &model.requests()[1].messages;
    let encoded = serde_json::to_string(second).unwrap();
    assert_eq!(encoded.matches("first prompt").count(), 1);
    assert_eq!(encoded.matches("first answer").count(), 1);
}

#[tokio::test]
async fn process_style_reopen_resumes_canonical_history_once() {
    let first_model = Arc::new(MockAdapter::text("persisted answer"));
    let first = test_engine(first_model, Config::default());
    first
        .run_turn(request("persisted prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();
    let id = first.session_manager.session_id.clone();
    let dir = first.session_manager.file_path.parent().unwrap().to_path_buf();
    drop(first);

    let resumed_store = SessionManager::new(&dir, Some(&id)).unwrap();
    let resumed_model = Arc::new(MockAdapter::text("resumed answer"));
    let resumed = test_engine_with_session(resumed_model.clone(), Config::default(), Some(resumed_store));
    resumed
        .run_turn(request("resume prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    let history = &resumed_model.requests()[0].messages;
    let encoded = serde_json::to_string(history).unwrap();
    assert_eq!(encoded.matches("persisted prompt").count(), 1);
    assert_eq!(encoded.matches("persisted answer").count(), 1);
}

async fn seed_rebuild_engine(config: Config) -> (crate::engine::AgentEngine, String) {
    let model = Arc::new(MockAdapter::text("stored answer"));
    let engine = test_engine(model, config);
    engine
        .run_turn(request("stored prompt"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();
    let id = engine.session_manager.session_id.clone();
    (engine, id)
}

#[tokio::test]
async fn rebuild_agent_from_session_history_resumes_without_duplication() {
    let (engine, id) = seed_rebuild_engine(Config::default()).await;
    let mut rebuilt_config = Config {
        sessions_dir: engine.session_manager.file_path.parent().unwrap().to_path_buf(),
        ..Config::default()
    };
    rebuilt_config.model = "mock-rebuilt".to_string();

    let rebuilt = crate::engine::AgentEngine::new(rebuilt_config, AuthStore::default(), Some(&id))
        .await
        .unwrap();
    let messages = rebuilt.session_manager.load_messages().await.unwrap();
    assert_eq!(messages.len(), 2);
}
