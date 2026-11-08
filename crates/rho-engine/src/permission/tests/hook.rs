use rho_harness_core::presentation::types::InteractionResponse;
use serde_json::json;
use std::sync::Arc;
use tempfile::tempdir;

use super::mock::MockHookPresenter;
use crate::permission::guard::GuardEvaluator;
use crate::permission::hook::{PermissionAction, PermissionHook};
use crate::permission::policy::{build_policy, parse_scope_from_str};
use crate::provider::adapter::MockAdapter;
use crate::tools::engine_tool::EngineTool;
use crate::tools::{EditTool, WriteTool};

#[tokio::test]
async fn test_allowed_call_runs_silently() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(true, None));
    let scope = parse_scope_from_str("[permission.bash]\n\"git *\" = \"allow\"\n").unwrap();
    let policy = build_policy(Some(scope), None);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter.clone(), policy);

    let verdict = hook.check_permission("bash", &json!({"command": "git status"})).await;
    assert_eq!(verdict, PermissionAction::Proceed);
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

    let verdict = hook
        .check_permission("bash", &json!({"command": "rm -rf /tmp/test"}))
        .await;
    assert_eq!(
        verdict,
        PermissionAction::Deny("denied by permission rule 'bash|rm -rf *'".to_string())
    );
}

#[tokio::test]
async fn test_ask_in_headless_mode_fails_closed() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(false, None));
    let policy = build_policy(None, None);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter, policy);

    let verdict = hook
        .check_permission("bash", &json!({"command": "touch test.txt"}))
        .await;
    match verdict {
        PermissionAction::Deny(msg) => {
            assert!(msg.contains("cannot prompt in headless mode"));
        }
        other => panic!("expected deny in headless mode, got {other:?}"),
    }
}

#[tokio::test]
async fn test_ask_interactive_allow_action() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(0))));
    let policy = build_policy(None, None);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter.clone(), policy);

    let verdict = hook
        .check_permission("bash", &json!({"command": "touch test.txt"}))
        .await;
    assert_eq!(verdict, PermissionAction::Proceed);
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

    let verdict = hook
        .check_permission("bash", &json!({"command": "touch original.txt"}))
        .await;
    match verdict {
        PermissionAction::RewriteArgs(args) => {
            assert_eq!(args.get("command").and_then(|c| c.as_str()), Some("echo edited"));
        }
        other => panic!("expected rewrite args, got {other:?}"),
    }
}

fn assert_permission_persisted(path: &std::path::Path, content: &str) {
    let perm_file = path.join("permission.toml");
    assert!(perm_file.exists());
    assert!(std::fs::read_to_string(&perm_file).unwrap().contains(content));
}

#[tokio::test]
async fn test_ask_interactive_always_allow_persists_and_updates_policy() {
    let _guard = super::ENV_LOCK.lock().await;
    let global_dir = tempdir().unwrap();
    unsafe {
        std::env::set_var("RHO_HOME", global_dir.path());
    }

    let project_dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(2))));
    let hook = PermissionHook::new(Some(project_dir.path().to_path_buf()), presenter.clone());

    let verdict = hook
        .check_permission("bash", &json!({"command": "touch persist_test"}))
        .await;
    assert_eq!(verdict, PermissionAction::Proceed);
    assert_permission_persisted(global_dir.path(), "touch persist_test");

    // Second execution should now be allowed automatically
    let verdict2 = hook
        .check_permission("bash", &json!({"command": "touch persist_test"}))
        .await;
    assert_eq!(verdict2, PermissionAction::Proceed);

    unsafe {
        std::env::remove_var("RHO_HOME");
    }
}

#[tokio::test]
async fn test_ask_interactive_always_allow_custom_pattern_persists() {
    let _guard = super::ENV_LOCK.lock().await;
    let global_dir = tempdir().unwrap();
    unsafe {
        std::env::set_var("RHO_HOME", global_dir.path());
    }

    let project_dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(
        true,
        Some(InteractionResponse::SelectedWithInput {
            index: 2,
            text: "touch custom_*".to_string(),
        }),
    ));
    let hook = PermissionHook::new(Some(project_dir.path().to_path_buf()), presenter.clone());

    let verdict = hook
        .check_permission("bash", &json!({"command": "touch custom_foo"}))
        .await;
    assert_eq!(verdict, PermissionAction::Proceed);
    assert_permission_persisted(global_dir.path(), "touch custom_*");

    unsafe {
        std::env::remove_var("RHO_HOME");
    }
}

#[tokio::test]
async fn test_ask_interactive_deny_with_reason() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(
        true,
        Some(InteractionResponse::SelectedWithInput {
            index: 3,
            text: "not safe".to_string(),
        }),
    ));
    let policy = build_policy(None, None);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter, policy);

    let verdict = hook
        .check_permission("bash", &json!({"command": "touch denied.txt"}))
        .await;
    assert_eq!(
        verdict,
        PermissionAction::Deny("Operation denied by user: not safe".to_string())
    );
}

#[tokio::test]
async fn test_ask_interactive_cancel() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Cancelled)));
    let policy = build_policy(None, None);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter, policy);

    let verdict = hook
        .check_permission("bash", &json!({"command": "touch cancel.txt"}))
        .await;
    assert_eq!(
        verdict,
        PermissionAction::Deny("Operation cancelled by user.".to_string())
    );
}

#[tokio::test]
async fn test_guard_evaluator_auto_allows_safe_command() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(true, None));
    let policy = build_policy(None, None);

    let guard_mock = Arc::new(MockAdapter::text(
        r#"{"safe": true, "reason": "Local directory creation"}"#,
    ));
    let guard = GuardEvaluator::new(guard_mock);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter.clone(), policy).with_guard(guard);

    let verdict = hook
        .check_permission("bash", &json!({"command": "mkdir -p src/foo"}))
        .await;
    assert_eq!(verdict, PermissionAction::Proceed);
    assert!(presenter.last_prompt.lock().unwrap().is_none());
}

#[tokio::test]
async fn test_guard_evaluator_prompts_unsafe_command_with_reason() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(0))));
    let policy = build_policy(None, None);

    let guard_mock = Arc::new(MockAdapter::text(
        r#"{"safe": false, "action": "Pushes local commits to remote main", "reason": "Git push modifies remote repository"}"#,
    ));
    let guard = GuardEvaluator::new(guard_mock);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter.clone(), policy).with_guard(guard);

    let verdict = hook
        .check_permission("bash", &json!({"command": "git push origin main"}))
        .await;
    assert_eq!(verdict, PermissionAction::Proceed);
    let prompt = presenter.last_prompt.lock().unwrap().clone().unwrap();
    assert!(prompt.body.contains("Notice: Pushes local commits to remote main"));
    assert!(prompt.body.contains("Risk: Git push modifies remote repository"));
    assert!(prompt.body.contains("\n\n\x1b[2mgit push origin main\x1b[0m\n"));
}

#[tokio::test]
async fn test_guard_evaluator_critical_danger_intercepts_without_model() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Cancelled)));
    let policy = build_policy(None, None);

    let guard_mock = Arc::new(MockAdapter::default());
    let guard = GuardEvaluator::new(guard_mock);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter.clone(), policy).with_guard(guard);

    let _ = hook
        .check_permission("bash", &json!({"command": "git reset --hard HEAD~1"}))
        .await;
    let prompt = presenter.last_prompt.lock().unwrap().clone().unwrap();
    assert!(
        prompt
            .body
            .contains("Notice: Resets git working tree and index discarding uncommitted changes")
    );
    assert!(
        prompt
            .body
            .contains("Risk: Irreversible loss of git working tree state")
    );
    assert!(prompt.body.contains("\n\n\x1b[2mgit reset --hard HEAD~1\x1b[0m\n"));
}

#[tokio::test]
async fn test_guard_evaluator_error_falls_back_to_prompt() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Cancelled)));
    let policy = build_policy(None, None);

    let guard_mock = Arc::new(MockAdapter::default());
    let guard = GuardEvaluator::new(guard_mock);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter.clone(), policy).with_guard(guard);

    let _ = hook.check_permission("bash", &json!({"command": "npm publish"})).await;
    let prompt = presenter.last_prompt.lock().unwrap().clone().unwrap();
    assert!(prompt.body.contains("npm publish"));
}

#[tokio::test]
async fn test_guard_evaluator_headless_denies_unsafe_command() {
    let dir = tempdir().unwrap();
    let presenter = Arc::new(MockHookPresenter::new(false, None));
    let policy = build_policy(None, None);

    let guard_mock = Arc::new(MockAdapter::text(
        r#"{"safe": false, "reason": "Cluster deletion is unsafe"}"#,
    ));
    let guard = GuardEvaluator::new(guard_mock);
    let hook = PermissionHook::with_policy(Some(dir.path().to_path_buf()), presenter.clone(), policy).with_guard(guard);

    let verdict = hook
        .check_permission("bash", &json!({"command": "kubectl delete pod foo"}))
        .await;
    match verdict {
        PermissionAction::Deny(msg) => {
            assert!(msg.contains("cannot prompt in headless mode"));
        }
        other => panic!("expected deny in headless mode, got {other:?}"),
    }
}

#[tokio::test]
async fn test_external_write_prompts_and_completes_on_allow() {
    let ws_dir = tempdir().unwrap();
    let ext_dir = tempdir().unwrap();
    let ext_file = ext_dir.path().join("hook_written.txt");

    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(0))));
    let policy = build_policy(None, None);
    let hook = PermissionHook::with_policy(Some(ws_dir.path().to_path_buf()), presenter.clone(), policy);

    let write_tool = WriteTool::new(ws_dir.path());
    let args = json!({
        "path": ext_file.to_string_lossy(),
        "content": "external write through hook"
    });

    let verdict = hook.check_permission("write", &args).await;
    assert_eq!(verdict, PermissionAction::Proceed);
    assert!(presenter.last_prompt.lock().unwrap().is_some());

    let res = (&write_tool as &dyn EngineTool).execute(args).await.unwrap();
    assert!(!res.is_error);
    assert_eq!(
        std::fs::read_to_string(&ext_file).unwrap(),
        "external write through hook"
    );
}

#[tokio::test]
async fn test_external_edit_prompts_and_completes_on_allow() {
    let ws_dir = tempdir().unwrap();
    let ext_dir = tempdir().unwrap();
    let ext_file = ext_dir.path().join("hook_edited.txt");
    std::fs::write(&ext_file, "before edit\n").unwrap();

    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(0))));
    let policy = build_policy(None, None);
    let hook = PermissionHook::with_policy(Some(ws_dir.path().to_path_buf()), presenter.clone(), policy);

    let edit_tool = EditTool::new(ws_dir.path());
    let args = json!({
        "path": ext_file.to_string_lossy(),
        "edits": [{
            "oldText": "before edit",
            "newText": "after edit"
        }]
    });

    let verdict = hook.check_permission("edit", &args).await;
    assert_eq!(verdict, PermissionAction::Proceed);
    assert!(presenter.last_prompt.lock().unwrap().is_some());

    let res = (&edit_tool as &dyn EngineTool).execute(args).await.unwrap();
    assert!(!res.is_error);
    assert_eq!(std::fs::read_to_string(&ext_file).unwrap(), "after edit\n");
}

#[tokio::test]
async fn test_always_allow_save_failure_denies_instead_of_running() {
    let _guard = super::ENV_LOCK.lock().await;
    let dir = tempdir().unwrap();
    let blocker = dir.path().join("not-a-dir");
    std::fs::write(&blocker, "").unwrap();
    unsafe {
        std::env::set_var("RHO_HOME", &blocker);
    }

    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(2))));
    let hook = PermissionHook::new(Some(dir.path().to_path_buf()), presenter.clone());
    let verdict = hook
        .check_permission("bash", &json!({"command": "touch never_run"}))
        .await;

    unsafe {
        std::env::remove_var("RHO_HOME");
    }
    assert!(
        matches!(&verdict, PermissionAction::Deny(msg) if msg.contains("Failed to save always-allow rule")),
        "{verdict:?}"
    );
    assert!(!dir.path().join("never_run").exists());
}

struct OverlapPresenter {
    in_flight: std::sync::atomic::AtomicUsize,
    max_in_flight: std::sync::atomic::AtomicUsize,
    prompts: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl rho_harness_core::presentation::presenter::Presenter for OverlapPresenter {
    fn write_output(&self, _text: &str) {}
    fn print_welcome(&self, _display: &rho_harness_core::presentation::WelcomeDisplay) {}
    fn print_session_status(&self, _display: &rho_harness_core::presentation::SessionStatus) {}
    fn print_notice(&self, _text: &str) {}
    fn print_block(&self, _display: &rho_harness_core::presentation::BlockDisplay) {}
    fn print_user_block(&self, _input: &str) {}
    fn print_token(&self, _token: &str) {}
    fn print_thinking_token(&self, _token: &str) {}
    fn finish_tool_line(&self, _line: rho_harness_core::presentation::ToolLine) {}
    fn flush(&self) {}
    fn has_interactive_ui(&self) -> bool {
        true
    }
    fn start_spinner(&self, _message: &str) -> rho_harness_core::presentation::activity::ActivityToken {
        Default::default()
    }
    fn start_tool_spinner(
        &self,
        _name: &str,
        _arguments: &serde_json::Value,
    ) -> rho_harness_core::presentation::activity::ActivityToken {
        Default::default()
    }
    fn start_tool_run(&self, _name: &str, _arguments: &serde_json::Value) {}
    fn stream_port(&self) -> rho_harness_core::presentation::stream::ToolStreamPort {
        Default::default()
    }
    async fn request_interaction(
        &self,
        _prompt: rho_harness_core::presentation::InteractionPrompt,
    ) -> Option<InteractionResponse> {
        use std::sync::atomic::Ordering::SeqCst;
        self.prompts.fetch_add(1, SeqCst);
        let now = self.in_flight.fetch_add(1, SeqCst) + 1;
        self.max_in_flight.fetch_max(now, SeqCst);
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        self.in_flight.fetch_sub(1, SeqCst);
        Some(InteractionResponse::Cancelled)
    }
}

#[tokio::test]
async fn test_concurrent_asks_prompt_one_at_a_time() {
    use std::sync::atomic::Ordering::SeqCst;
    let dir = tempdir().unwrap();
    let presenter = Arc::new(OverlapPresenter {
        in_flight: Default::default(),
        max_in_flight: Default::default(),
        prompts: Default::default(),
    });
    let hook = PermissionHook::with_policy(
        Some(dir.path().to_path_buf()),
        presenter.clone(),
        build_policy(None, None),
    );
    let args = json!({"command": "make deploy"});

    let (a, b, c) = tokio::join!(
        hook.check_permission("bash", &args),
        hook.check_permission("bash", &args),
        hook.check_permission("bash", &args),
    );

    for verdict in [a, b, c] {
        assert!(matches!(verdict, PermissionAction::Deny(_)));
    }
    assert_eq!(presenter.prompts.load(SeqCst), 3);
    assert_eq!(presenter.max_in_flight.load(SeqCst), 1);
}
