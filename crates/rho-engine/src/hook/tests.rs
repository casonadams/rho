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

#[test]
fn test_find_hook_project_agents_directory() {
    let project = tempfile::tempdir().unwrap();
    let hooks_dir = project.path().join(".agents").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let hook_file = hooks_dir.join("on_tool_call");
    std::fs::write(&hook_file, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook_file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let found = find_hook_in_paths("tool_call", project.path(), None);
    assert_eq!(found, Some(hook_file));

    let claude_project = tempfile::tempdir().unwrap();
    let claude_file = claude_project.path().join(".claude/hooks/on_turn_start");
    std::fs::create_dir_all(claude_file.parent().unwrap()).unwrap();
    std::fs::write(&claude_file, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&claude_file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(
        find_hook_in_paths("turn_start", claude_project.path(), None),
        Some(claude_file)
    );
}

#[test]
fn test_find_hook_user_fallback() {
    let project = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();

    let user_hooks = user.path().join(".agents").join("hooks");
    std::fs::create_dir_all(&user_hooks).unwrap();
    let hook_file = user_hooks.join("turn_start");
    std::fs::write(&hook_file, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook_file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let found = find_hook_in_paths("turn_start", project.path(), Some(user.path()));
    assert_eq!(found, Some(hook_file));
}

#[test]
fn test_find_hook_project_overrides_user() {
    let project = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();

    let project_hooks = project.path().join(".agents").join("hooks");
    std::fs::create_dir_all(&project_hooks).unwrap();
    let project_hook = project_hooks.join("on_tool_call");
    std::fs::write(&project_hook, "#!/bin/sh\nexit 0\n").unwrap();

    let user_hooks = user.path().join(".agents").join("hooks");
    std::fs::create_dir_all(&user_hooks).unwrap();
    let user_hook = user_hooks.join("on_tool_call");
    std::fs::write(&user_hook, "#!/bin/sh\nexit 0\n").unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&project_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&user_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let found = find_hook_in_paths("tool_call", project.path(), Some(user.path()));
    assert_eq!(found, Some(project_hook));
}

#[test]
fn test_find_hook_ignores_legacy_rho_directory() {
    let project = tempfile::tempdir().unwrap();
    let legacy_hooks = project.path().join(".rho").join("hooks");
    std::fs::create_dir_all(&legacy_hooks).unwrap();
    let hook_file = legacy_hooks.join("on_tool_call");
    std::fs::write(&hook_file, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook_file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let found = find_hook_in_paths("tool_call", project.path(), None);
    assert_eq!(found, None);
}

#[cfg(unix)]
#[test]
fn test_find_hook_requires_executable_bit_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let project = tempfile::tempdir().unwrap();
    let hooks_dir = project.path().join(".agents").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let hook_file = hooks_dir.join("on_tool_call");
    std::fs::write(&hook_file, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&hook_file, std::fs::Permissions::from_mode(0o644)).unwrap();

    let found = find_hook_in_paths("tool_call", project.path(), None);
    assert_eq!(found, None);
}

#[cfg(unix)]
#[test]
fn test_find_all_hooks_is_additive_with_hook_dirs() {
    use std::os::unix::fs::PermissionsExt;
    let project = tempfile::tempdir().unwrap();
    let project_hooks = project.path().join(".agents").join("hooks");
    std::fs::create_dir_all(&project_hooks).unwrap();
    let project_hook = project_hooks.join("turn_end");
    std::fs::write(&project_hook, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&project_hook, std::fs::Permissions::from_mode(0o755)).unwrap();

    let extra_dir = tempfile::tempdir().unwrap();
    let extra_hook = extra_dir.path().join("turn_end");
    std::fs::write(&extra_hook, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&extra_hook, std::fs::Permissions::from_mode(0o755)).unwrap();

    let hook = LifecycleHook::new(
        project.path().to_path_buf(),
        "s1",
        Arc::new(crate::engine::eval::presenter::NoopPresenter),
    )
    .with_hook_dirs(vec![extra_dir.path().to_path_buf()]);

    let all = hook.find_all_hooks("turn_end");
    assert_eq!(all.len(), 2, "both extra hook-dir and project hook are found");
    assert_eq!(all[0], extra_hook);
    assert_eq!(all[1], project_hook);
}
