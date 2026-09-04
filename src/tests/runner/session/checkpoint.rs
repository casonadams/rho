use super::super::helpers::{final_event, presenter, request, test_engine, test_engine_with_session};
use crate::config::Config;
use crate::error::AppError;
use crate::session::SessionManager;
use crate::ui::TerminalRenderer;
use rig::completion::Usage;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

#[tokio::test]
async fn budget_exhausted_checkpoint_survives_process_resume_and_promotes_once() {
    let first_model = MockCompletionModel::from_stream_turns([
        [
            MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path":"missing-a"})),
            final_event(Usage::new()),
        ],
        [
            MockStreamEvent::tool_call("call-2", "read", serde_json::json!({"path":"missing-b"})),
            final_event(Usage::new()),
        ],
    ]);
    let first = test_engine(
        first_model,
        Config {
            auto_approve: true,
            max_turns: 2,
            ..Config::default()
        },
    );
    let error = first
        .run_turn(
            request("inspect the repository"),
            presenter(&TerminalRenderer::default()),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::ModelBudgetExhausted { max_turns: 2 }));
    assert!(first.session_manager.load_messages().await.unwrap().is_empty());
    let checkpoint = first.session_manager.load_checkpoint().await.unwrap().unwrap();
    assert_eq!(checkpoint.len(), 5);
    let id = first.session_manager.session_id.clone();
    let dir = first.session_manager.file_path.parent().unwrap().to_path_buf();
    drop(first);

    let resumed_store = SessionManager::new(&dir, Some(&id)).unwrap();
    let resumed_model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::text("repository summary"),
        final_event(Usage::new()),
    ]]);
    let resumed = test_engine_with_session(
        resumed_model.clone(),
        Config {
            auto_approve: true,
            max_turns: 2,
            ..Config::default()
        },
        Some(resumed_store),
    );
    resumed
        .run_turn(request("please continue"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap();

    let history = &resumed_model.requests()[0].chat_history;
    let encoded = serde_json::to_string(history).unwrap();
    assert_eq!(encoded.matches("inspect the repository").count(), 1);
    assert_eq!(encoded.matches("missing-a").count(), 2);
    assert_eq!(encoded.matches("missing-b").count(), 2);
    assert_eq!(encoded.matches("please continue").count(), 1);
    assert!(resumed.session_manager.load_checkpoint().await.unwrap().is_none());
    assert_eq!(resumed.session_manager.load_messages().await.unwrap().len(), 7);

    drop(resumed);
    let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
    assert!(reopened.load_checkpoint().await.unwrap().is_none());
    assert_eq!(reopened.load_messages().await.unwrap().len(), 7);
}

#[tokio::test]
async fn failed_checkpoint_continuation_remains_available_until_success() {
    let probe_path = "checkpoint-probe-missing-3f9b";
    let first_model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("call-1", "read", serde_json::json!({"path": probe_path})),
        final_event(Usage::new()),
    ]]);
    let first = test_engine(
        first_model,
        Config {
            max_turns: 1,
            ..Config::default()
        },
    );
    first
        .run_turn(request("inspect"), presenter(&TerminalRenderer::default()))
        .await
        .unwrap_err();
    let checkpoint = first.session_manager.load_checkpoint().await.unwrap().unwrap();
    let id = first.session_manager.session_id.clone();
    let dir = first.session_manager.file_path.parent().unwrap().to_path_buf();
    drop(first);

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
    for req in resumed_model.requests() {
        let history = serde_json::to_string(&req.chat_history).unwrap();
        assert_eq!(
            history
                .matches(r#""role":"user","content":[{"type":"text","text":"inspect"}"#)
                .count(),
            1
        );
        assert_eq!(history.matches(probe_path).count(), 2);
    }
    assert!(resumed.session_manager.load_checkpoint().await.unwrap().is_none());

    drop(resumed);
    let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
    assert!(reopened.load_checkpoint().await.unwrap().is_none());
    assert_eq!(reopened.load_messages().await.unwrap().len(), 5);
}
