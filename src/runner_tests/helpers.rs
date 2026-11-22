use crate::config::Config;
use crate::engine::AgentEngine;
use crate::engine::runner::TurnRequest;
use crate::session::SessionManager;
use crate::ui::TerminalRenderer;
use rho_engine::provider::adapter::{MockAdapter, ModelAdapter};
use rho_engine::tools::EngineTool;
use rho_harness_core::presentation::presenter::Presenter;
use rho_test_support::TestDir;
use std::ops::Deref;
use std::sync::Arc;

pub fn presenter(renderer: &TerminalRenderer) -> Arc<dyn Presenter> {
    Arc::new(renderer.clone())
}

pub struct TestEngine {
    engine: AgentEngine,
    dir: Option<TestDir>,
}

impl TestEngine {
    /// Drops the engine and returns the scratch directory so a test can reopen the stored session.
    pub fn into_dir(self) -> TestDir {
        self.dir.expect("test_engine owns a scratch directory")
    }
}

impl Deref for TestEngine {
    type Target = AgentEngine;

    fn deref(&self) -> &AgentEngine {
        &self.engine
    }
}

pub fn test_engine(model: Arc<MockAdapter>, app_config: Config) -> TestEngine {
    let dir = TestDir::with_prefix("runner_test_");
    let sessions_dir = dir.join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();
    let session_manager = SessionManager::new(&sessions_dir, None).unwrap();
    let mut app_config = Config {
        sessions_dir,
        ..app_config
    };
    app_config.permission.enabled = false;
    TestEngine {
        dir: Some(dir),
        ..test_engine_with_session(model, app_config, Some(session_manager))
    }
}

pub fn test_engine_with_session(
    model: Arc<MockAdapter>,
    app_config: Config,
    session_manager: Option<SessionManager>,
) -> TestEngine {
    let scratch = session_manager.is_none().then(|| TestDir::with_prefix("runner_test_"));
    let base_dir = match (&session_manager, &scratch) {
        (Some(session), _) => session.file_path.parent().unwrap().to_path_buf(),
        (None, Some(dir)) => dir.to_path_buf(),
        (None, None) => unreachable!("a scratch directory exists whenever there is no session"),
    };
    let engine = rho_engine::engine::eval::mock::mock_engine(
        model as Arc<dyn ModelAdapter>,
        rho_engine::engine::eval::mock::MockEngineConfig {
            base_dir: &base_dir,
            app_config: Config {
                sessions_dir: base_dir.join("sessions"),
                ..app_config
            },
            session_manager,
            built_in_tools: builtin_tools_for(&base_dir),
        },
    );
    TestEngine { engine, dir: scratch }
}

pub fn builtin_tools_for(base_dir: &std::path::Path) -> Option<Vec<Arc<dyn EngineTool>>> {
    rho_engine::tools::build_builtin_tools(
        base_dir,
        &Config {
            sessions_dir: base_dir.join("sessions"),
            ..Config::default()
        },
        None,
    )
    .ok()
}

pub fn terminal_session() -> (SessionManager, TestDir) {
    let dir = TestDir::with_prefix("sink_test_");
    (SessionManager::new(&dir, None).unwrap(), dir)
}

pub fn request(prompt: &str) -> TurnRequest<'_> {
    TurnRequest::new(prompt)
}
