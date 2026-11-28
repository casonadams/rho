use super::*;
use crate::engine::metrics::TerminalStatus;
use test_case::test_case;

#[test]
fn test_hook_event_serde() {
    let event = HookEvent::ToolCall {
        tool_name: "bash".to_string(),
        args: serde_json::json!({"command": "ls"}),
        turn: 1,
        session_id: "s123".to_string(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"event\":\"tool_call\""));
    assert!(json.contains("\"tool_name\":\"bash\""));

    let deserialized: HookEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, event);
}

#[test_case(TerminalStatus::Completed, "completed"; "completed")]
#[test_case(TerminalStatus::ContentFiltered, "content_filtered"; "content filtered")]
#[test_case(TerminalStatus::BudgetExhausted, "budget_exhausted"; "budget exhausted")]
#[test_case(TerminalStatus::Cancelled, "cancelled"; "cancelled")]
#[test_case(TerminalStatus::Failed, "failed"; "failed")]
fn turn_end_payload_status_is_the_terminal_status_serde_name(status: TerminalStatus, expected: &str) {
    let event = HookEvent::TurnEnd {
        status,
        tool_calls_count: 0,
        turn: 1,
        session_id: "s1".to_string(),
        error: None,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["status"], expected);
    assert!(json.get("error").is_none());
}

#[test_case(r#"{"action":"continue"}"#, HookAction::Continue; "continue")]
#[test_case(r#"{"action":"stop","reason":"test"}"#, HookAction::Stop { reason: "test".to_string() }; "stop")]
#[test_case(r#"{"action":"skip","reason":"skip it"}"#, HookAction::Skip { reason: "skip it".to_string() }; "skip")]
#[test_case(
    r#"{"action":"rewrite_args","args":{"a":1}}"#,
    HookAction::RewriteArgs { args: serde_json::json!({"a": 1}) };
    "rewrite args"
)]
#[test_case(
    r#"{"action":"rewrite_result","result":"foo"}"#,
    HookAction::RewriteResult { result: "foo".to_string() };
    "rewrite result"
)]
#[test_case(r#"{"action":"ask","message":"confirm?"}"#, HookAction::Ask { message: "confirm?".to_string() }; "ask")]
#[test_case(
    r#"{"action":"retry","feedback":"bad args"}"#,
    HookAction::Retry { feedback: "bad args".to_string() };
    "retry"
)]
fn test_hook_action_serde(raw: &str, expected: HookAction) {
    let parsed: HookAction = serde_json::from_str(raw).unwrap();
    assert_eq!(parsed, expected);
}

#[test]
fn test_parse_hook_output() {
    // 1. Successful execution returning stop action
    let action = parse_hook_output(true, Some(0), br#"{"action":"stop","reason":"blocked"}"#, b"").unwrap();
    assert_eq!(
        action,
        HookAction::Stop {
            reason: "blocked".to_string()
        }
    );

    // 2. Successful execution exiting 0 with no stdout returns Continue
    let action = parse_hook_output(true, Some(0), b"", b"").unwrap();
    assert_eq!(action, HookAction::Continue);

    // 3. Script exiting non-zero but with action JSON still returns the action
    let action = parse_hook_output(false, Some(1), br#"{"action":"stop","reason":"blocked"}"#, b"").unwrap();
    assert_eq!(
        action,
        HookAction::Stop {
            reason: "blocked".to_string()
        }
    );

    // 4. Script failing with non-zero exit and no action returns error with stderr
    let res = parse_hook_output(false, Some(1), b"", b"fatal error");
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("fatal error"));
    assert!(err.contains("status 1"));

    // 5. Successful execution with malformed JSON returns parse error
    let res = parse_hook_output(true, Some(0), b"invalid json", b"");
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Failed to parse hook response"));
}

fn write_plain_script(path: &Path) {
    crate::test_support::write_file(path, "#!/bin/sh\nexit 0\n");
}

fn install_plain_hook(root: &Path, event: &str) -> std::path::PathBuf {
    let hook = root.join(".agents").join("hooks").join(event);
    write_plain_script(&hook);
    hook
}

#[cfg(unix)]
fn set_unix_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(unix)]
fn write_executable(path: &Path) {
    write_plain_script(path);
    set_unix_mode(path, 0o755);
}

#[cfg(unix)]
fn install_hook(root: &Path, event: &str) -> std::path::PathBuf {
    let hook = install_plain_hook(root, event);
    set_unix_mode(&hook, 0o755);
    hook
}

fn first_hook(event: &str, dirs: &[&Path]) -> Option<std::path::PathBuf> {
    collect_hooks(event, &[], &[], dirs).into_iter().next()
}

fn assert_no_hook_found(setup: impl FnOnce(&Path)) {
    let project = crate::test_support::TestDir::new();
    setup(&project);
    assert_eq!(first_hook("tool_call", &[&project]), None);
}

#[cfg(unix)]
#[test]
fn test_find_hook_project_agents_directory() {
    let project = crate::test_support::TestDir::new();
    let hook_file = project.join(".agents").join("hooks").join("on_tool_call");
    write_executable(&hook_file);

    let found = collect_hooks("tool_call", &[], &[], &[&project]).into_iter().next();
    assert_eq!(found, Some(hook_file));

    let claude_project = crate::test_support::TestDir::new();
    let claude_file = claude_project.join(".claude/hooks/on_turn_start");
    write_executable(&claude_file);
    assert_eq!(
        collect_hooks("turn_start", &[], &[], &[&claude_project])
            .into_iter()
            .next(),
        Some(claude_file)
    );
}

#[cfg(unix)]
#[test]
fn test_find_hook_user_fallback() {
    let user = crate::test_support::TestDir::new();
    let hook_file = install_hook(&user, "turn_start");

    let found = collect_hooks("turn_start", &[], &[&user], &[]);
    assert_eq!(found, vec![hook_file]);
}

#[cfg(unix)]
#[test]
fn test_project_hook_is_skipped_when_workspace_untrusted() {
    let project = crate::test_support::TestDir::new();
    install_hook(&project, "tool_call");
    let user = crate::test_support::TestDir::new();

    assert_eq!(collect_hooks("tool_call", &[], &[&user], &[]), Vec::<PathBuf>::new());
}

#[cfg(unix)]
#[test]
fn test_user_claude_hooks_directory_is_ignored() {
    let user = crate::test_support::TestDir::new();
    write_executable(&user.join(".claude/hooks/on_turn_start"));

    assert_eq!(collect_hooks("turn_start", &[], &[&user], &[]), Vec::<PathBuf>::new());
}

#[test]
fn test_find_hook_ignores_legacy_rho_directory() {
    assert_no_hook_found(|root| {
        let hook_file = root.join(".rho/hooks/on_tool_call");
        write_plain_script(&hook_file);
        #[cfg(unix)]
        set_unix_mode(&hook_file, 0o755);
    });
}

#[cfg(unix)]
#[test]
fn test_find_hook_requires_executable_bit_on_unix() {
    assert_no_hook_found(|root| {
        let hook_file = install_plain_hook(root, "tool_call");
        set_unix_mode(&hook_file, 0o644);
    });
}

#[cfg(unix)]
#[test]
fn test_hook_precedence_user_then_project_then_configured_dirs() {
    let project = crate::test_support::TestDir::new();
    let project_hook = project.join(".agents").join("hooks").join("turn_end");
    write_executable(&project_hook);

    let user = crate::test_support::TestDir::new();
    let user_hook = user.join(".agents").join("hooks").join("turn_end");
    write_executable(&user_hook);

    let found = collect_hooks("turn_end", &[], &[&user], &[&project]);
    assert_eq!(found, vec![user_hook.clone(), project_hook.clone()]);

    let extra_dir = crate::test_support::TestDir::new();
    let extra_hook = extra_dir.join("turn_end");
    write_executable(&extra_hook);

    let all = collect_hooks("turn_end", &[extra_dir.to_path_buf()], &[&user], &[&project]);
    assert_eq!(all, vec![extra_hook, user_hook, project_hook]);
}

#[cfg(unix)]
async fn outcome_with_hook_script(script: &str) -> ToolOutcomeAction {
    let dir = crate::test_support::TestDir::new();
    let hook = dir.join("tool_result");
    crate::test_support::write_file(&hook, script);
    set_unix_mode(&hook, 0o755);
    let lifecycle = LifecycleHook::new(
        dir.to_path_buf(),
        "s1",
        std::sync::Arc::new(crate::engine::eval::presenter::NoopPresenter),
    )
    .with_hook_dirs(vec![dir.to_path_buf()]);
    lifecycle
        .on_tool_outcome("read", &serde_json::json!({}), "SECRET=hunter2", false)
        .await
}

#[cfg(unix)]
#[tokio::test]
async fn a_failing_tool_result_hook_withholds_the_raw_output() {
    let action = outcome_with_hook_script("#!/bin/sh\necho boom >&2\nexit 3\n").await;
    let ToolOutcomeAction::RewriteResult(text) = action else {
        panic!("expected the output to be replaced");
    };
    assert!(text.contains("withheld") && text.contains("boom"), "{text}");
    assert!(!text.contains("hunter2"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_tool_result_hook_that_continues_leaves_the_output_alone() {
    let action = outcome_with_hook_script("#!/bin/sh\nexit 0\n").await;
    assert!(matches!(action, ToolOutcomeAction::Proceed));
}
