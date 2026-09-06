use super::common::HistoryTerminal;
use crate::engine::builder::AgentEngineBuilder;
use crate::repl::live::navigation::update_footer;
use crate::repl::live::turn::sync_turn_footer;
use crate::ui::interactive::{Activity, InteractiveState, RunningTool, TerminalController};
use rho_engine::auth::AuthStore;
use rho_harness_core::config::Config;

async fn test_engine_for_footer() -> (crate::engine::AgentEngine, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let config = Config {
        provider: "local".to_string(),
        model: "llama3.2".to_string(),
        sessions_dir: temp.path().join("sessions"),
        ..Default::default()
    };
    let engine = AgentEngineBuilder::new(config, AuthStore::default())
        .build()
        .await
        .unwrap();
    (engine, temp)
}

#[tokio::test]
async fn sync_turn_footer_updates_in_flight_tokens_and_detects_changes() {
    let (engine, _temp) = test_engine_for_footer().await;
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();

    assert!(sync_turn_footer(&mut controller, &engine) && !sync_turn_footer(&mut controller, &engine));

    engine.usage().start_turn(Some(500));
    assert!(sync_turn_footer(&mut controller, &engine));
    assert_eq!(controller.state().footer().total_input_tokens, 500);

    engine.usage().record_streaming_chunk(25);
    assert!(sync_turn_footer(&mut controller, &engine));
    assert_eq!(controller.state().footer().total_output_tokens, 25);

    let usage = rho_engine::engine::metrics::StructuralUsage {
        input_tokens: 520,
        output_tokens: 30,
        total_tokens: 550,
        cached_input_tokens: Some(100),
        cache_creation_input_tokens: None,
        tool_use_prompt_tokens: None,
        reasoning_tokens: None,
    };
    engine.usage().record_step(usage, 400);

    assert!(sync_turn_footer(&mut controller, &engine));
    let f = controller.state().footer();
    assert_eq!(
        (f.total_input_tokens, f.total_output_tokens, f.total_cache_read_tokens),
        (520, 30, 100)
    );
}

#[tokio::test]
async fn update_footer_resets_activity_running_tool_and_active_tool() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config {
        provider: "local".to_string(),
        model: "llama3.2".to_string(),
        sessions_dir: temp.path().join("sessions"),
        ..Default::default()
    };
    let auth_store = AuthStore::default();
    let engine = AgentEngineBuilder::new(config.clone(), auth_store.clone())
        .build()
        .await
        .unwrap();
    let session = crate::repl::ReplSession::new(config, auth_store, None);

    let mut state = InteractiveState::default();
    state.footer_mut().activity = Activity::Working;
    state.footer_mut().running_tool = Some("bash".to_string());
    state.set_active_tool(Some(RunningTool::new("bash".to_string(), "sleep 30".to_string(), None)));

    update_footer(&mut state, &session, &engine);

    assert_eq!(state.footer().activity, Activity::Idle);
    assert_eq!(state.footer().running_tool, None);
    assert!(state.active_tool().is_none());
}
