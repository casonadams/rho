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
    assert_eq!(
        extract_path_argument(&json!({"path": "src/lib.rs"})),
        Some("src/lib.rs")
    );
    assert_eq!(
        extract_path_argument(&json!({"file_path": "crates/foo/bar.rs"})),
        Some("crates/foo/bar.rs")
    );
    assert_eq!(
        extract_path_argument(&json!({"filePath": "nested/path.rs"})),
        Some("nested/path.rs")
    );
    assert_eq!(
        extract_path_argument(&json!({"path": "  \"quoted/path.rs\"  "})),
        Some("quoted/path.rs")
    );
    assert_eq!(
        extract_path_argument(&json!({"path": "  'single_quoted.rs'  "})),
        Some("single_quoted.rs")
    );
    assert_eq!(extract_path_argument(&json!({"path": ""})), None);
    assert_eq!(extract_path_argument(&json!({"other": 123})), None);
    assert_eq!(extract_path_argument(&json!({})), None);
    assert_eq!(extract_path_argument(&json!(null)), None);
}

#[tokio::test]
async fn test_tool_hook_dynamic_subtree_activation_during_turn() {
    let temp = tempfile::tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let plugin_crate = repo_root.join("crates").join("rho-plugin-sdk");
    let plugin_src = plugin_crate.join("src");

    tokio::fs::create_dir_all(repo_root.join(".git")).await.unwrap();
    tokio::fs::create_dir_all(&plugin_src).await.unwrap();

    let root_agents = repo_root.join("AGENTS.md");
    tokio::fs::write(&root_agents, "# Root Workspace Instructions\n")
        .await
        .unwrap();

    let plugin_agents = plugin_crate.join("AGENTS.md");
    tokio::fs::write(&plugin_agents, "# Plugin SDK Subtree Instructions\n")
        .await
        .unwrap();

    let lib_rs = plugin_src.join("lib.rs");
    tokio::fs::write(&lib_rs, "pub fn hello() {}").await.unwrap();

    let initial_ctx = ProjectContext::discover(&repo_root, None).await;
    assert_eq!(initial_ctx.instruction_files.len(), 1);
    assert_eq!(initial_ctx.instruction_files[0].1, "# Root Workspace Instructions");

    let shared_ctx = Arc::new(Mutex::new(Some((repo_root.clone(), initial_ctx))));

    let hook =
        TurnToolExecutionHook::new(mock_sink(&repo_root), "anthropic", None).with_project_context(shared_ctx.clone());

    let relative_file_path = "crates/rho-plugin-sdk/src/lib.rs";
    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "read", json!({"path": relative_file_path})),
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
    assert_eq!(updated_ctx.instruction_files[0].1, "# Root Workspace Instructions");
    assert_eq!(updated_ctx.instruction_files[1].1, "# Plugin SDK Subtree Instructions");

    let subsequent_prompt = updated_ctx.build_system_prompt();
    assert!(subsequent_prompt.contains("# Root Workspace Instructions"));
    assert!(subsequent_prompt.contains("# Plugin SDK Subtree Instructions"));
    let root_idx = subsequent_prompt.find("# Root Workspace Instructions").unwrap();
    let plugin_idx = subsequent_prompt.find("# Plugin SDK Subtree Instructions").unwrap();
    assert!(root_idx < plugin_idx);
}
