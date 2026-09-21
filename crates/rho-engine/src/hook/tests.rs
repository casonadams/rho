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

#[cfg(unix)]
#[tokio::test]
async fn test_run_hook_script_execution() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let script_path = temp.path().join("test_hook.sh");

    // 1. Script returning stop action
    std::fs::write(
        &script_path,
        "#!/bin/sh\necho '{\"action\":\"stop\",\"reason\":\"blocked\"}'\n",
    )
    .unwrap();
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();

    let event = HookEvent::ToolCall {
        tool_name: "bash".to_string(),
        args: serde_json::json!({"command": "rm -rf /"}),
        turn: 1,
        session_id: "s1".to_string(),
    };

    let action = run_hook(&script_path, &event, temp.path(), DEFAULT_HOOK_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(
        action,
        HookAction::Stop {
            reason: "blocked".to_string()
        }
    );

    // 2. Script exiting 0 with no stdout returns Continue
    std::fs::write(&script_path, "#!/bin/sh\nexit 0\n").unwrap();
    let action = run_hook(&script_path, &event, temp.path(), DEFAULT_HOOK_TIMEOUT)
        .await
        .unwrap();
    assert_eq!(action, HookAction::Continue);

    // 3. Script failing returns error
    std::fs::write(&script_path, "#!/bin/sh\necho 'fatal error' >&2\nexit 1\n").unwrap();
    let res = run_hook(&script_path, &event, temp.path(), DEFAULT_HOOK_TIMEOUT).await;
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("fatal error"));

    // 4. Script timing out fails closed
    std::fs::write(&script_path, "#!/bin/sh\nsleep 10\n").unwrap();
    let res = run_hook(&script_path, &event, temp.path(), std::time::Duration::from_millis(50)).await;
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("timed out"));
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
