use super::*;
use serde_json::{Value, json};

async fn call(tool: &TerminalTool, args: Value) -> ToolResult {
    let args: TerminalArgs = serde_json::from_value(args).unwrap();
    tool.run(args).await.unwrap()
}

async fn ok(tool: &TerminalTool, args: Value) -> String {
    let result = call(tool, args).await;
    assert!(!result.is_error, "{}", result.content);
    result.content
}

#[tokio::test]
async fn start_returns_the_numbered_screen_and_the_exit_status() {
    let dir = tempfile::tempdir().unwrap();
    let tool = TerminalTool::new(dir.path());

    let screen = ok(
        &tool,
        json!({ "action": "start", "command": "printf 'hello\\n\\nworld  \\n'", "cols": 30, "rows": 6 }),
    )
    .await;

    assert!(screen.starts_with("[terminal 1: printf"), "{screen}");
    assert!(screen.contains(" 30x6, cursor 4:1, exited 0]\n"), "{screen}");
    assert!(screen.ends_with("\n1|hello\n2|\n3|world"), "{screen}");
}

#[tokio::test]
async fn sent_text_and_keys_reach_the_program() {
    let dir = tempfile::tempdir().unwrap();
    let tool = TerminalTool::new(dir.path());
    ok(
        &tool,
        json!({ "action": "start", "command": "cat", "cols": 40, "rows": 5, "wait_ms": 300 }),
    )
    .await;

    let echoed = ok(
        &tool,
        json!({ "action": "send", "id": 1, "text": "ping", "keys": ["Enter"], "wait_for": "ping\nping" }),
    )
    .await;
    assert!(echoed.contains("\n1|ping\n2|ping"), "{echoed}");

    let interrupted = ok(&tool, json!({ "action": "send", "id": 1, "keys": ["C-c"] })).await;
    assert!(interrupted.contains(", terminated by "), "{interrupted}");

    let after_exit = call(&tool, json!({ "action": "send", "id": 1, "text": "x" })).await;
    assert!(
        after_exit.is_error && after_exit.content.contains("has exited"),
        "{}",
        after_exit.content
    );
    let closed = ok(&tool, json!({ "action": "close", "id": 1 })).await;
    assert!(closed.contains("Closed: terminated by"), "{closed}");
    assert_eq!(ok(&tool, json!({ "action": "list" })).await, "No terminal sessions.");
}

#[tokio::test]
async fn resize_reaches_the_program() {
    let dir = tempfile::tempdir().unwrap();
    let tool = TerminalTool::new(dir.path());
    ok(
        &tool,
        json!({ "action": "start", "command": "stty size; read line; stty size", "cols": 50, "rows": 10, "wait_for": "10 50" }),
    )
    .await;

    let resized = ok(&tool, json!({ "action": "resize", "id": 1, "cols": 70, "rows": 20 })).await;
    assert!(resized.contains(" 70x20, "), "{resized}");
    let screen = ok(
        &tool,
        json!({ "action": "send", "id": 1, "keys": ["Enter"], "wait_for": "20 70" }),
    )
    .await;

    assert!(screen.contains("|10 50\n") && screen.contains("|20 70"), "{screen}");
}

#[tokio::test]
async fn closing_kills_a_running_program() {
    let dir = tempfile::tempdir().unwrap();
    let tool = TerminalTool::new(dir.path());
    ok(&tool, json!({ "action": "start", "command": "cat", "wait_ms": 10 })).await;

    let closed = ok(&tool, json!({ "action": "close", "id": 1 })).await;

    assert!(
        closed.starts_with("[terminal 1: cat 120x40, cursor 1:1, running]"),
        "{closed}"
    );
    assert!(closed.contains("\nClosed: terminated by"), "{closed}");
    let again = call(&tool, json!({ "action": "screen", "id": 1 })).await;
    assert!(
        again.is_error && again.content.contains("no terminal session 1"),
        "{}",
        again.content
    );
}

#[tokio::test]
async fn a_missing_wait_for_text_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let tool = TerminalTool::new(dir.path());

    let screen = ok(
        &tool,
        json!({ "action": "start", "command": "cat", "wait_ms": 100, "wait_for": "never" }),
    )
    .await;

    assert!(screen.ends_with("[\"never\" did not appear within 100 ms]"), "{screen}");
}

#[tokio::test]
async fn sessions_are_capped() {
    let dir = tempfile::tempdir().unwrap();
    let tool = TerminalTool::new(dir.path());
    for _ in 0..MAX_SESSIONS {
        ok(&tool, json!({ "action": "start", "command": "cat", "wait_ms": 1 })).await;
    }

    let refused = call(&tool, json!({ "action": "start", "command": "cat" })).await;

    assert!(
        refused.is_error && refused.content.contains("close one"),
        "{}",
        refused.content
    );
}

#[tokio::test]
async fn invalid_requests_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let tool = TerminalTool::new(dir.path());
    let cases = [
        (json!({ "action": "start" }), "start needs a command"),
        (
            json!({ "action": "start", "command": "cat", "cols": 0 }),
            "out of range",
        ),
        (json!({ "action": "screen" }), "needs the session id"),
        (
            json!({ "action": "send", "id": 9, "text": "x" }),
            "no terminal session 9",
        ),
    ];
    for (args, expected) in cases {
        let result = call(&tool, args.clone()).await;
        assert!(
            result.is_error && result.content.contains(expected),
            "{args}: {}",
            result.content
        );
    }

    ok(&tool, json!({ "action": "start", "command": "cat", "wait_ms": 1 })).await;
    for (args, expected) in [
        (json!({ "action": "send", "id": 1 }), "send needs text or keys"),
        (
            json!({ "action": "send", "id": 1, "keys": ["Hyper"] }),
            "unknown key 'Hyper'",
        ),
        (
            json!({ "action": "resize", "id": 1, "cols": 80 }),
            "resize needs cols and rows",
        ),
    ] {
        let result = call(&tool, args.clone()).await;
        assert!(
            result.is_error && result.content.contains(expected),
            "{args}: {}",
            result.content
        );
    }
}
