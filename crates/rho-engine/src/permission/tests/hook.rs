use crate::test_support::TestDir;
use rho_harness_core::presentation::types::InteractionResponse;
use rho_test_support::EnvVarGuard;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

use super::mock::MockHookPresenter;
use crate::permission::guard::GuardEvaluator;
use crate::permission::hook::{PermissionAction, PermissionHook};
use crate::permission::policy::{build_policy, parse_scope_from_str};
use crate::provider::adapter::MockAdapter;
use crate::tools::edit::EditTool;
use crate::tools::engine_tool::EngineTool;
use crate::tools::write::WriteTool;

fn workspace_hook(dir: &Path, presenter: Arc<MockHookPresenter>) -> PermissionHook {
    PermissionHook::with_policy(Some(dir.to_path_buf()), presenter, build_policy(None, None))
}

fn guarded_hook(dir: &Path, presenter: Arc<MockHookPresenter>, guard_model: Arc<MockAdapter>) -> PermissionHook {
    workspace_hook(dir, presenter).with_guard(GuardEvaluator::new(guard_model))
}

async fn check_bash(hook: &PermissionHook, command: &str) -> PermissionAction {
    hook.check_permission("bash", &json!({ "command": command })).await
}

fn last_prompt_body(presenter: &MockHookPresenter) -> String {
    presenter.last_prompt.lock().unwrap().clone().unwrap().body
}

fn assert_headless_denial(verdict: PermissionAction) {
    assert!(
        matches!(&verdict, PermissionAction::Deny(msg) if msg.contains("cannot prompt in headless mode")),
        "{verdict:?}"
    );
}

#[tokio::test]
async fn test_allowed_call_runs_silently() {
    let dir = TestDir::new();
    let presenter = Arc::new(MockHookPresenter::new(true, None));
    let scope = parse_scope_from_str("[permission.bash]\n\"git *\" = \"allow\"\n").unwrap();
    let policy = build_policy(Some(scope), None);
    let hook = PermissionHook::with_policy(Some(dir.to_path_buf()), presenter.clone(), policy);

    let verdict = hook.check_permission("bash", &json!({"command": "git status"})).await;
    assert_eq!(verdict, PermissionAction::Proceed);
    assert!(presenter.last_prompt.lock().unwrap().is_none());
}

#[tokio::test]
async fn test_denied_call_skipped_with_reason() {
    let dir = TestDir::new();
    let scope = parse_scope_from_str(
        r#"
[permission.bash]
"rm -rf *" = "deny"
"#,
    )
    .unwrap();
    let policy = build_policy(None, Some(scope));
    let presenter = Arc::new(MockHookPresenter::new(true, None));
    let hook = PermissionHook::with_policy(Some(dir.to_path_buf()), presenter, policy);

    let verdict = check_bash(&hook, "rm -rf /tmp/test").await;
    assert_eq!(
        verdict,
        PermissionAction::Deny("denied by permission rule 'bash|rm -rf *'".to_string())
    );
}

#[tokio::test]
async fn a_hook_for_a_worktree_treats_the_worktree_as_the_workspace() {
    let main = TestDir::new();
    let worktree = TestDir::new();
    let presenter = Arc::new(MockHookPresenter::new(false, None));
    let hook = workspace_hook(&main, presenter);
    let agent = hook.for_working_dir(worktree.to_path_buf());
    let write = |path: std::path::PathBuf| json!({ "path": path.to_string_lossy(), "content": "x" });

    let inside = agent.check_permission("write", &write(worktree.join("a.rs"))).await;
    let relative = agent
        .check_permission("write", &json!({ "path": "src/a.rs", "content": "x" }))
        .await;
    let into_main = agent.check_permission("write", &write(main.join("a.rs"))).await;

    assert_eq!(inside, PermissionAction::Proceed);
    assert_eq!(relative, PermissionAction::Proceed);
    assert!(matches!(into_main, PermissionAction::Deny(_)), "{into_main:?}");
}

#[tokio::test]
async fn test_ask_in_headless_mode_fails_closed() {
    let dir = TestDir::new();
    let hook = workspace_hook(&dir, Arc::new(MockHookPresenter::new(false, None)));

    assert_headless_denial(check_bash(&hook, "touch test.txt").await);
}

#[tokio::test]
async fn test_ask_interactive_allow_action() {
    let dir = TestDir::new();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(0))));
    let hook = workspace_hook(&dir, presenter.clone());

    let verdict = check_bash(&hook, "touch test.txt").await;
    assert_eq!(verdict, PermissionAction::Proceed);
    assert!(presenter.last_prompt.lock().unwrap().is_some());
}

#[tokio::test]
async fn test_ask_interactive_edit_action() {
    let dir = TestDir::new();
    let presenter = Arc::new(MockHookPresenter::new(
        true,
        Some(InteractionResponse::SelectedWithInput {
            index: 1,
            text: "echo edited".to_string(),
        }),
    ));
    let hook = workspace_hook(&dir, presenter);

    let verdict = check_bash(&hook, "touch original.txt").await;
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
    let global_dir = TestDir::new();
    let _env = EnvVarGuard::set("RHO_HOME", &global_dir.to_string_lossy());

    let project_dir = TestDir::new();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(2))));
    let hook = PermissionHook::new(Some(project_dir.to_path_buf()), presenter.clone());

    let verdict = check_bash(&hook, "touch persist_test").await;
    assert_eq!(verdict, PermissionAction::Proceed);
    assert_permission_persisted(&global_dir, "touch persist_test");

    // Second execution should now be allowed automatically
    let verdict2 = check_bash(&hook, "touch persist_test").await;
    assert_eq!(verdict2, PermissionAction::Proceed);
}

#[tokio::test]
async fn malformed_global_policy_is_announced_when_the_hook_loads() {
    let global_dir = TestDir::new();
    std::fs::write(global_dir.join("permission.toml"), "[permission\n").unwrap();
    let _env = EnvVarGuard::set("RHO_HOME", &global_dir.to_string_lossy());

    let presenter = Arc::new(crate::test_support::CapturingPresenter::default());
    let _hook = PermissionHook::new(None, presenter.clone());

    let notices = presenter.notices.lock().unwrap();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("permission.toml"), "{notices:?}");
}

#[tokio::test]
async fn test_ask_interactive_always_allow_custom_pattern_persists() {
    let global_dir = TestDir::new();
    let _env = EnvVarGuard::set("RHO_HOME", &global_dir.to_string_lossy());

    let project_dir = TestDir::new();
    let presenter = Arc::new(MockHookPresenter::new(
        true,
        Some(InteractionResponse::SelectedWithInput {
            index: 2,
            text: "touch custom_*".to_string(),
        }),
    ));
    let hook = PermissionHook::new(Some(project_dir.to_path_buf()), presenter.clone());

    let verdict = check_bash(&hook, "touch custom_foo").await;
    assert_eq!(verdict, PermissionAction::Proceed);
    assert_permission_persisted(&global_dir, "touch custom_*");
}

#[tokio::test]
async fn test_ask_interactive_deny_with_reason() {
    let dir = TestDir::new();
    let presenter = Arc::new(MockHookPresenter::new(
        true,
        Some(InteractionResponse::SelectedWithInput {
            index: 3,
            text: "not safe".to_string(),
        }),
    ));
    let hook = workspace_hook(&dir, presenter);

    let verdict = check_bash(&hook, "touch denied.txt").await;
    assert_eq!(
        verdict,
        PermissionAction::Deny("Operation denied by user: not safe".to_string())
    );
}

#[tokio::test]
async fn test_ask_interactive_cancel() {
    let dir = TestDir::new();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Cancelled)));
    let hook = workspace_hook(&dir, presenter);

    let verdict = check_bash(&hook, "touch cancel.txt").await;
    assert_eq!(
        verdict,
        PermissionAction::Deny("Operation cancelled by user.".to_string())
    );
}

#[tokio::test]
async fn test_guard_evaluator_auto_allows_safe_command() {
    let dir = TestDir::new();
    let presenter = Arc::new(MockHookPresenter::new(true, None));
    let guard_model = Arc::new(MockAdapter::text(
        r#"{"safe": true, "reason": "Local directory creation"}"#,
    ));
    let hook = guarded_hook(&dir, presenter.clone(), guard_model);

    let verdict = check_bash(&hook, "mkdir -p src/foo").await;
    assert_eq!(verdict, PermissionAction::Proceed);
    assert!(presenter.last_prompt.lock().unwrap().is_none());
}

#[tokio::test]
async fn test_guard_evaluator_prompts_unsafe_command_with_reason() {
    let dir = TestDir::new();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(0))));
    let guard_model = Arc::new(MockAdapter::text(
        r#"{"safe": false, "action": "Pushes local commits to remote main", "reason": "Git push modifies remote repository"}"#,
    ));
    let hook = guarded_hook(&dir, presenter.clone(), guard_model);

    let verdict = check_bash(&hook, "git push origin main").await;
    assert_eq!(verdict, PermissionAction::Proceed);
    let body = last_prompt_body(&presenter);
    assert!(body.contains("Notice: Pushes local commits to remote main"));
    assert!(body.contains("Risk: Git push modifies remote repository"));
    assert!(body.contains("\n\n\x1b[2mgit push origin main\x1b[0m\n"));
}

#[tokio::test]
async fn test_guard_evaluator_critical_danger_intercepts_without_model() {
    let dir = TestDir::new();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Cancelled)));
    let hook = guarded_hook(&dir, presenter.clone(), Arc::new(MockAdapter::default()));

    let _ = check_bash(&hook, "git reset --hard HEAD~1").await;
    let body = last_prompt_body(&presenter);
    assert!(body.contains("Notice: Resets git working tree and index discarding uncommitted changes"));
    assert!(body.contains("Risk: Irreversible loss of git working tree state"));
    assert!(body.contains("\n\n\x1b[2mgit reset --hard HEAD~1\x1b[0m\n"));
}

#[tokio::test]
async fn test_guard_evaluator_error_falls_back_to_prompt() {
    let dir = TestDir::new();
    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Cancelled)));
    let hook = guarded_hook(&dir, presenter.clone(), Arc::new(MockAdapter::default()));

    let _ = check_bash(&hook, "npm publish").await;
    assert!(last_prompt_body(&presenter).contains("npm publish"));
}

#[tokio::test]
async fn test_guard_evaluator_headless_denies_unsafe_command() {
    let dir = TestDir::new();
    let presenter = Arc::new(MockHookPresenter::new(false, None));
    let guard_model = Arc::new(MockAdapter::text(
        r#"{"safe": false, "reason": "Cluster deletion is unsafe"}"#,
    ));
    let hook = guarded_hook(&dir, presenter, guard_model);

    assert_headless_denial(check_bash(&hook, "kubectl delete pod foo").await);
}

#[tokio::test]
async fn test_external_write_prompts_and_completes_on_allow() {
    let ws_dir = TestDir::new();
    let ext_dir = TestDir::new();
    let ext_file = ext_dir.join("hook_written.txt");

    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(0))));
    let hook = workspace_hook(&ws_dir, presenter.clone());

    let write_tool = WriteTool::new(&ws_dir);
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
    let ws_dir = TestDir::new();
    let ext_dir = TestDir::new();
    let ext_file = ext_dir.join("hook_edited.txt");
    std::fs::write(&ext_file, "before edit\n").unwrap();

    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(0))));
    let hook = workspace_hook(&ws_dir, presenter.clone());

    let edit_tool = EditTool::new(&ws_dir);
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
    let dir = TestDir::new();
    let blocker = dir.join("not-a-dir");
    std::fs::write(&blocker, "").unwrap();
    let _env = EnvVarGuard::set("RHO_HOME", &blocker.to_string_lossy());

    let presenter = Arc::new(MockHookPresenter::new(true, Some(InteractionResponse::Selected(2))));
    let hook = PermissionHook::new(Some(dir.to_path_buf()), presenter.clone());
    let verdict = check_bash(&hook, "touch never_run").await;

    assert!(
        matches!(&verdict, PermissionAction::Deny(msg) if msg.contains("Failed to save always-allow rule")),
        "{verdict:?}"
    );
    assert!(!dir.join("never_run").exists());
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
    let dir = TestDir::new();
    let presenter = Arc::new(OverlapPresenter {
        in_flight: Default::default(),
        max_in_flight: Default::default(),
        prompts: Default::default(),
    });
    let hook = PermissionHook::with_policy(Some(dir.to_path_buf()), presenter.clone(), build_policy(None, None));
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
