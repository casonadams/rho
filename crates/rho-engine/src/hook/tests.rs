use super::*;

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

#[test]
fn test_hook_action_serde() {
    let actions = [
        (r#"{"action":"continue"}"#, HookAction::Continue),
        (
            r#"{"action":"stop","reason":"test"}"#,
            HookAction::Stop {
                reason: "test".to_string(),
            },
        ),
        (
            r#"{"action":"skip","reason":"skip it"}"#,
            HookAction::Skip {
                reason: "skip it".to_string(),
            },
        ),
        (
            r#"{"action":"rewrite_args","args":{"a":1}}"#,
            HookAction::RewriteArgs {
                args: serde_json::json!({"a": 1}),
            },
        ),
        (
            r#"{"action":"rewrite_result","result":"foo"}"#,
            HookAction::RewriteResult {
                result: "foo".to_string(),
            },
        ),
        (
            r#"{"action":"ask","message":"confirm?"}"#,
            HookAction::Ask {
                message: "confirm?".to_string(),
            },
        ),
        (
            r#"{"action":"retry","feedback":"bad args"}"#,
            HookAction::Retry {
                feedback: "bad args".to_string(),
            },
        ),
    ];

    for (raw, expected) in actions {
        let parsed: HookAction = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed, expected);
    }
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
    let project = tempfile::tempdir().unwrap();
    setup(project.path());
    assert_eq!(first_hook("tool_call", &[project.path()]), None);
}

#[cfg(unix)]
#[test]
fn test_find_hook_project_agents_directory() {
    let project = tempfile::tempdir().unwrap();
    let hook_file = project.path().join(".agents").join("hooks").join("on_tool_call");
    write_executable(&hook_file);

    let found = collect_hooks("tool_call", &[], &[], &[project.path()])
        .into_iter()
        .next();
    assert_eq!(found, Some(hook_file));

    let claude_project = tempfile::tempdir().unwrap();
    let claude_file = claude_project.path().join(".claude/hooks/on_turn_start");
    write_executable(&claude_file);
    assert_eq!(
        collect_hooks("turn_start", &[], &[], &[claude_project.path()])
            .into_iter()
            .next(),
        Some(claude_file)
    );
}

#[cfg(unix)]
#[test]
fn test_find_hook_user_fallback() {
    let user = tempfile::tempdir().unwrap();
    let hook_file = install_hook(user.path(), "turn_start");

    let found = collect_hooks("turn_start", &[], &[user.path()], &[]);
    assert_eq!(found, vec![hook_file]);
}

#[cfg(unix)]
#[test]
fn test_project_hook_is_skipped_when_workspace_untrusted() {
    let project = tempfile::tempdir().unwrap();
    install_hook(project.path(), "tool_call");
    let user = tempfile::tempdir().unwrap();

    assert_eq!(
        collect_hooks("tool_call", &[], &[user.path()], &[]),
        Vec::<PathBuf>::new()
    );
}

#[cfg(unix)]
#[test]
fn test_user_claude_hooks_directory_is_ignored() {
    let user = tempfile::tempdir().unwrap();
    write_executable(&user.path().join(".claude/hooks/on_turn_start"));

    assert_eq!(
        collect_hooks("turn_start", &[], &[user.path()], &[]),
        Vec::<PathBuf>::new()
    );
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
    let project = tempfile::tempdir().unwrap();
    let project_hook = project.path().join(".agents").join("hooks").join("turn_end");
    write_executable(&project_hook);

    let user = tempfile::tempdir().unwrap();
    let user_hook = user.path().join(".agents").join("hooks").join("turn_end");
    write_executable(&user_hook);

    let found = collect_hooks("turn_end", &[], &[user.path()], &[project.path()]);
    assert_eq!(found, vec![user_hook.clone(), project_hook.clone()]);

    let extra_dir = tempfile::tempdir().unwrap();
    let extra_hook = extra_dir.path().join("turn_end");
    write_executable(&extra_hook);

    let all = collect_hooks(
        "turn_end",
        &[extra_dir.path().to_path_buf()],
        &[user.path()],
        &[project.path()],
    );
    assert_eq!(all, vec![extra_hook, user_hook, project_hook]);
}
