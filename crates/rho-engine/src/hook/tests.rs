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
