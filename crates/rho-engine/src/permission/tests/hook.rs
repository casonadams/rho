use rho_harness_core::presentation::types::InteractionResponse;
use rig::agent::AgentBuilder;
use rig::test_utils::{MockCompletionModel, MockTurn};
use serde_json::json;
use std::sync::Arc;
use tempfile::tempdir;

use super::mock::MockHookPresenter;
use crate::permission::hook::PermissionHook;
use crate::permission::policy::{build_policy, parse_scope_from_str};
use crate::tools::BashTool;

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn test_allowed_call_runs_silently() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(true, None));
    let policy = build_policy(None, None);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter.clone(), policy);

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "bash", json!({"command": "git status"})),
        MockTurn::text("success"),
    ]);

    let agent = AgentBuilder::new(model.clone())
        .tool(BashTool::new(dir.path()))
        .add_hook(hook)
        .build();

    let response = agent.runner("status").max_turns(2).run().await.unwrap();
    assert_eq!(response.output, "success");
    assert!(presenter.last_prompt.lock().unwrap().is_none());
}

#[tokio::test]
async fn test_denied_call_skipped_with_reason() {
    let dir = tempdir().unwrap();
    let scope = parse_scope_from_str(
        r#"
[permission.bash]
"rm -rf *" = "deny"
"#,
    )
    .unwrap();
    let policy = build_policy(None, Some(scope));
    let presenter = Arc::new(MockHookPresenter::new(true, None));
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter, policy);

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "bash", json!({"command": "rm -rf /tmp/test"})),
        MockTurn::text("stopped"),
    ]);

    let agent = AgentBuilder::new(model.clone())
        .tool(BashTool::new(dir.path()))
        .add_hook(hook)
        .build();

    let response = agent.runner("delete").max_turns(2).run().await.unwrap();
    assert_eq!(response.output, "stopped");
    let history = format!("{:?}", model.requests()[1].chat_history);
    assert!(history.contains("denied by permission rule 'bash|rm -rf *'"));
}

#[tokio::test]
async fn test_ask_in_headless_mode_fails_closed() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(false, None));
    let policy = build_policy(None, None);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter, policy);

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "bash", json!({"command": "touch test.txt"})),
        MockTurn::text("headless denied"),
    ]);

    let agent = AgentBuilder::new(model.clone())
        .tool(BashTool::new(dir.path()))
        .add_hook(hook)
        .build();

    let response = agent.runner("run").max_turns(2).run().await.unwrap();
    assert_eq!(response.output, "headless denied");
    let history = format!("{:?}", model.requests()[1].chat_history);
    assert!(history.contains("cannot prompt in headless mode"));
}

#[tokio::test]
async fn test_ask_interactive_allow_action() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(0))));
    let policy = build_policy(None, None);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter.clone(), policy);

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "bash", json!({"command": "touch test.txt"})),
        MockTurn::text("allowed"),
    ]);

    let agent = AgentBuilder::new(model)
        .tool(BashTool::new(dir.path()))
        .add_hook(hook)
        .build();

    let response = agent.runner("touch").max_turns(2).run().await.unwrap();
    assert_eq!(response.output, "allowed");
    assert!(presenter.last_prompt.lock().unwrap().is_some());
}

#[tokio::test]
async fn test_ask_interactive_edit_action() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(
        true,
        Some(InteractionResponse::SelectedWithInput {
            index: 1,
            text: "echo edited".to_string(),
        }),
    ));
    let policy = build_policy(None, None);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter, policy);

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "bash", json!({"command": "touch original.txt"})),
        MockTurn::text("done"),
    ]);

    let agent = AgentBuilder::new(model.clone())
        .tool(BashTool::new(dir.path()))
        .add_hook(hook)
        .build();

    let response = agent.runner("run").max_turns(2).run().await.unwrap();
    assert_eq!(response.output, "done");
    let history = format!("{:?}", model.requests()[1].chat_history);
    assert!(history.contains("edited"));
}

#[tokio::test]
async fn test_ask_interactive_always_allow_persists_and_updates_policy() {
    let _guard = ENV_LOCK.lock().await;
    let global_dir = tempdir().unwrap();
    unsafe {
        std::env::set_var("RHO_HOME", global_dir.path());
    }

    let project_dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(2))));
    let hook = PermissionHook::new(Some(project_dir.path().to_path_buf()), presenter.clone());

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "bash", json!({"command": "touch persist_test"})),
        MockTurn::tool_call("2", "bash", json!({"command": "touch persist_test"})),
        MockTurn::text("completed"),
    ]);

    let agent = AgentBuilder::new(model)
        .tool(BashTool::new(project_dir.path()))
        .add_hook(hook)
        .build();

    let response = agent.runner("persist").max_turns(3).run().await.unwrap();
    assert_eq!(response.output, "completed");

    let perm_file = project_dir.path().join(".rho/permission.toml");
    assert!(perm_file.exists());
    let content = std::fs::read_to_string(&perm_file).unwrap();
    assert!(content.contains("touch persist_test"));

    unsafe {
        std::env::remove_var("RHO_HOME");
    }
}

#[tokio::test]
async fn test_ask_interactive_deny_with_reason_and_cancel() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(
        true,
        Some(InteractionResponse::SelectedWithInput {
            index: 3,
            text: "not safe".to_string(),
        }),
    ));
    let policy = build_policy(None, None);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter, policy.clone());

    let model = MockCompletionModel::new([
        MockTurn::tool_call("1", "bash", json!({"command": "touch denied.txt"})),
        MockTurn::text("stopped"),
    ]);

    let agent = AgentBuilder::new(model.clone())
        .tool(BashTool::new(dir.path()))
        .add_hook(hook)
        .build();

    let _ = agent.runner("touch").max_turns(2).run().await.unwrap();
    let history = format!("{:?}", model.requests()[1].chat_history);
    assert!(history.contains("Operation denied by user: not safe"));

    let cancel_presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Cancelled)));
    let cancel_hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), cancel_presenter, policy);
    let cancel_model = MockCompletionModel::new([
        MockTurn::tool_call("1", "bash", json!({"command": "touch cancel.txt"})),
        MockTurn::text("stopped"),
    ]);
    let cancel_agent = AgentBuilder::new(cancel_model.clone())
        .tool(BashTool::new(dir.path()))
        .add_hook(cancel_hook)
        .build();
    let _ = cancel_agent.runner("touch").max_turns(2).run().await.unwrap();
    let cancel_history = format!("{:?}", cancel_model.requests()[1].chat_history);
    assert!(cancel_history.contains("Operation denied by user."));
}
