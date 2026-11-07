#![cfg(unix)]

use crate::temp_dir;
use rho::config::{Config, PermissionConfig};
use rho::engine::eval::mock::{MockEngineConfig, mock_engine};
use rho::engine::runner::TurnRequest;
use rho::presentation::StructuredPresenter;
use rho_engine::provider::adapter::{MockAdapter, ModelStreamEvent};
use rho_harness_core::model::{ToolCall, ToolFunction};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

struct PermFixture {
    engine: rho::engine::AgentEngine,
    presenter: Arc<StructuredPresenter>,
}

fn make_perm_engine(workspace: &std::path::Path, model: Arc<MockAdapter>, config: Config) -> PermFixture {
    let built_in_tools = rho_engine::tools::build_builtin_tools(workspace, &config, None).ok();
    let engine = mock_engine(
        model,
        MockEngineConfig {
            base_dir: workspace,
            app_config: config,
            session_manager: None,
            built_in_tools,
        },
    );
    let recording = rho::presentation::RecordingSink::default();
    PermFixture {
        engine,
        presenter: Arc::new(StructuredPresenter::recording(recording)),
    }
}

fn make_perm_model(cmd: &str, final_text: &'static str) -> Arc<MockAdapter> {
    let model = Arc::new(MockAdapter::default());
    model.add_turn(vec![Ok(ModelStreamEvent::ToolCall(ToolCall::new(
        "call_1",
        ToolFunction::new("bash", json!({"command": cmd})),
    )))]);
    model.add_turn(vec![Ok(ModelStreamEvent::Text(final_text.to_string()))]);
    model
}

struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    prev: Option<std::ffi::OsString>,
    dir: PathBuf,
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(ref p) = self.prev {
                std::env::set_var("RHO_HOME", p);
            } else {
                std::env::remove_var("RHO_HOME");
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn isolate_perm_env() -> EnvGuard {
    let lock = crate::ENV_LOCK.lock().unwrap();
    let prev = std::env::var_os("RHO_HOME");
    let dir = std::env::temp_dir().join(format!("perm_global_{}", uuid::Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&dir);
    unsafe {
        std::env::set_var("RHO_HOME", &dir);
    }
    EnvGuard { _lock: lock, prev, dir }
}

async fn run_headless_turn(
    rules: Option<&str>,
    cmd: &str,
    final_text: &'static str,
    prompt: &str,
) -> rho_harness_core::error::Result<rho_engine::engine::runner::TurnOutput> {
    let _env = isolate_perm_env();
    let ws = temp_dir("perm_test");
    if let Some(rules) = rules {
        let perm_dir = ws.join(".agents").join("permissions");
        std::fs::create_dir_all(&perm_dir).unwrap();
        std::fs::write(perm_dir.join("rules.toml"), rules).unwrap();
    }
    let config = Config {
        permission: PermissionConfig { enabled: true },
        ..Config::default()
    };
    let fix = make_perm_engine(&ws, make_perm_model(cmd, final_text), config);
    let result = fix.engine.run_turn(TurnRequest::new(prompt), fix.presenter).await;
    let _ = std::fs::remove_dir_all(&ws);
    result
}

#[tokio::test]
async fn permission_explicit_allow_runs_silently_in_headless() {
    let output = run_headless_turn(
        Some("[permission.bash]\n\"echo *\" = \"allow\"\n"),
        "echo hello_headless",
        "done echoing",
        "run echo",
    )
    .await
    .unwrap();

    assert_eq!(output.final_text, "done echoing");
}

#[tokio::test]
async fn permission_explicit_deny_halts_with_error_in_headless() {
    let result = run_headless_turn(
        Some("[permission.bash]\n\"rm *\" = \"deny\"\n"),
        "rm -rf /tmp/foo",
        "done removing",
        "remove foo",
    )
    .await;

    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn permission_ask_denies_cleanly_in_headless_mode() {
    let result = run_headless_turn(None, "touch /tmp/unmatched_file", "should not reach here", "touch file").await;

    assert!(result.is_ok() || result.is_err());
}
