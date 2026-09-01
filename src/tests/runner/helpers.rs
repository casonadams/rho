use crate::config::Config;
use crate::engine::AgentEngine;
use crate::engine::runner::TurnRequest;
use crate::session::SessionManager;
use crate::ui::TerminalRenderer;
use rho_core::presentation::presenter::Presenter;
use rig::completion::Usage;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use std::sync::Arc;

pub fn presenter(renderer: &TerminalRenderer) -> Arc<dyn Presenter> {
    Arc::new(renderer.clone())
}

pub fn test_engine(model: MockCompletionModel, app_config: Config) -> AgentEngine {
    let dir = std::env::temp_dir().join(format!("runner_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let session_manager = SessionManager::new(&dir, None).unwrap();
    test_engine_with_session(model, app_config, Some(session_manager))
}

pub fn test_engine_with_session(
    model: MockCompletionModel,
    app_config: Config,
    session_manager: Option<SessionManager>,
) -> AgentEngine {
    let base_dir = session_manager
        .as_ref()
        .map(|session| session.file_path.parent().unwrap().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join(format!("runner_test_{}", uuid::Uuid::new_v4())));
    rho_engine::engine::eval::mock::mock_engine_with_session(
        model,
        rho_engine::engine::eval::mock::MockEngineConfig {
            base_dir: &base_dir,
            app_config: Config {
                sessions_dir: base_dir.join("sessions"),
                ..app_config
            },
            session_manager,
            built_in_tools: builtin_tools_for(&base_dir),
        },
    )
}

pub fn builtin_tools_for(base_dir: &std::path::Path) -> Option<Vec<rig::tool::DynamicTool>> {
    rho_engine::tools::build_builtin_tools(
        base_dir,
        &Config {
            sessions_dir: base_dir.join("sessions"),
            ..Config::default()
        },
    )
    .ok()
}

pub fn terminal_session() -> SessionManager {
    let dir = std::env::temp_dir().join(format!("sink_test_{}", uuid::Uuid::new_v4()));
    SessionManager::new(&dir, None).unwrap()
}

pub fn final_event(usage: Usage) -> MockStreamEvent {
    MockStreamEvent::final_response(usage)
}

pub fn request(prompt: &str) -> TurnRequest<'_> {
    TurnRequest::new(prompt)
}
