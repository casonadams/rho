use super::super::{TurnToolExecutionHook, extract_path_argument};
use crate::engine::context::ProjectContext;
use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
use rho_harness_core::session::SessionManager;
use rig::agent::AgentBuilder;
use rig::test_utils::{MockCompletionModel, MockTurn};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

fn mock_sink(dir: &Path) -> Arc<TerminalApprovalSink> {
    let session = SessionManager::new(dir, None).unwrap();
    TerminalApprovalSink::new(
        &crate::engine::eval::presenter::presenter(),
        TerminalSinkConfig {
            model_label: "test-model".to_string(),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
    )
}

#[test]
fn test_extract_path_argument_variants() {
    let cases = [
        (json!({"path": "src/lib.rs"}), Some("src/lib.rs")),
        (json!({"file_path": "crates/foo/bar.rs"}), Some("crates/foo/bar.rs")),
        (json!({"filePath": "nested/path.rs"}), Some("nested/path.rs")),
        (json!({"path": "  \"quoted/path.rs\"  "}), Some("quoted/path.rs")),
        (json!({"path": "  'single_quoted.rs'  "}), Some("single_quoted.rs")),
        (json!({"path": ""}), None),
        (json!({"other": 123}), None),
        (json!({}), None),
        (json!(null), None),
    ];
    for (arg, expected) in cases {
        assert_eq!(extract_path_argument(&arg), expected);
    }
}

fn setup_subtree_repo(repo_root: &std::path::Path) {
    let plugin_crate = repo_root.join("crates").join("rho-plugin-sdk");
    let plugin_src = plugin_crate.join("src");
    std::fs::create_dir_all(repo_root.join(".git")).unwrap();
    std::fs::create_dir_all(&plugin_src).unwrap();
    std::fs::write(repo_root.join("AGENTS.md"), "# Root Workspace Instructions\n").unwrap();
    std::fs::write(plugin_crate.join("AGENTS.md"), "# Plugin SDK Subtree Instructions\n").unwrap();
    std::fs::write(plugin_src.join("lib.rs"), "pub fn hello() {}").unwrap();
}

#[tokio::test]
async fn test_tool_hook_dynamic_subtree_activation_during_turn() {
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    setup_subtree_repo(&repo_root);

    let initial_ctx = ProjectContext::discover(&repo_root, None).await;
    assert_eq!(initial_ctx.instruction_files.len(), 1);

    let shared_ctx = Arc::new(Mutex::new(Some((repo_root.clone(), initial_ctx))));
    let hook =
        TurnToolExecutionHook::new(mock_sink(&repo_root), "anthropic", None).with_project_context(shared_ctx.clone());

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "read", json!({"path": "crates/rho-plugin-sdk/src/lib.rs"})),
        MockTurn::text("file inspected"),
    ]);

    let agent = AgentBuilder::new(model)
        .tool(crate::tools::ReadTool::new(&repo_root))
        .add_hook(hook)
        .record_content_telemetry(false)
        .build();
    let response = agent.runner("Inspect plugin sdk").max_turns(3).run().await.unwrap();
    assert_eq!(response.output, "file inspected");

    let guard = shared_ctx.lock().await;
    let (_, updated_ctx) = guard.as_ref().unwrap();
    assert_eq!(updated_ctx.instruction_files.len(), 2);
    assert_eq!(updated_ctx.instruction_files[1].1, "# Plugin SDK Subtree Instructions");
}
