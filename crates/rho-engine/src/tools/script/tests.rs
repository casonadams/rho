use super::runner::{ScriptDispatcher, execute_script};
use rho_harness_core::args::{ScriptArgs, ScriptStep};
use std::sync::Arc;

fn sample_dispatcher() -> ScriptDispatcher {
    let mut dispatcher = ScriptDispatcher::new();
    dispatcher.register(
        "echo",
        Arc::new(|args: serde_json::Value| async move {
            let msg = args.get("msg").and_then(|v| v.as_str()).unwrap_or("hello");
            Ok(format!("output: {msg}"))
        }),
    );
    dispatcher.register(
        "multiline",
        Arc::new(|_args: serde_json::Value| async move {
            Ok("line 1: apple\nline 2: banana\nline 3: cherry\nline 4: date\nline 5: elderberry".to_string())
        }),
    );
    dispatcher.register(
        "failing",
        Arc::new(|_args: serde_json::Value| async move { Err("connection timed out".to_string()) }),
    );
    dispatcher
}

#[tokio::test]
async fn test_empty_script_errors() {
    let dispatcher = sample_dispatcher();
    let res = execute_script(&dispatcher, ScriptArgs { steps: Vec::new() }, 1024)
        .await
        .unwrap();
    assert!(res.is_error);
    assert!(res.content.contains("No steps provided"));
}

#[tokio::test]
async fn test_single_step_without_filter() {
    let dispatcher = sample_dispatcher();
    let steps = vec![ScriptStep {
        tool: "echo".to_string(),
        args: serde_json::json!({ "msg": "world" }),
        filter: None,
        context: None,
    }];
    let res = execute_script(&dispatcher, ScriptArgs { steps }, 1024).await.unwrap();
    assert!(!res.is_error);
    assert!(res.content.contains("[Step 1: echo]"));
    assert!(res.content.contains("output: world"));
}

#[tokio::test]
async fn test_step_with_filter_and_context() {
    let dispatcher = sample_dispatcher();
    let steps = vec![ScriptStep {
        tool: "multiline".to_string(),
        args: serde_json::json!({}),
        filter: Some("cherry".to_string()),
        context: Some(1),
    }];
    let res = execute_script(&dispatcher, ScriptArgs { steps }, 1024).await.unwrap();
    assert!(!res.is_error);
    assert!(res.content.contains("[Step 1: multiline | grep -C 1 \"cherry\"]"));
    assert!(res.content.contains("2-line 2: banana"));
    assert!(res.content.contains("3:line 3: cherry"));
    assert!(res.content.contains("4-line 4: date"));
    assert!(!res.content.contains("elderberry"));
}

#[tokio::test]
async fn test_multistep_script_runs_sequentially() {
    let dispatcher = sample_dispatcher();
    let steps = vec![
        ScriptStep {
            tool: "echo".to_string(),
            args: serde_json::json!({ "msg": "first" }),
            filter: None,
            context: None,
        },
        ScriptStep {
            tool: "multiline".to_string(),
            args: serde_json::json!({}),
            filter: Some("date".to_string()),
            context: Some(0),
        },
    ];
    let res = execute_script(&dispatcher, ScriptArgs { steps }, 1024).await.unwrap();
    assert!(!res.is_error);
    assert!(res.content.contains("[Step 1: echo]"));
    assert!(res.content.contains("output: first"));
    assert!(res.content.contains("[Step 2: multiline | grep \"date\"]"));
    assert!(res.content.contains("4:line 4: date"));
}

#[tokio::test]
async fn test_failure_short_circuits() {
    let dispatcher = sample_dispatcher();
    let steps = vec![
        ScriptStep {
            tool: "echo".to_string(),
            args: serde_json::json!({ "msg": "first" }),
            filter: None,
            context: None,
        },
        ScriptStep {
            tool: "failing".to_string(),
            args: serde_json::json!({}),
            filter: None,
            context: None,
        },
        ScriptStep {
            tool: "echo".to_string(),
            args: serde_json::json!({ "msg": "should not run" }),
            filter: None,
            context: None,
        },
    ];
    let res = execute_script(&dispatcher, ScriptArgs { steps }, 1024).await.unwrap();
    assert!(res.is_error);
    assert!(res.content.contains("[Step 1: echo]"));
    assert!(res.content.contains("[Step 2: failing Failed]"));
    assert!(res.content.contains("connection timed out"));
    assert!(!res.content.contains("should not run"));
}

#[tokio::test]
async fn test_nested_script_is_rejected() {
    let dispatcher = sample_dispatcher();
    let steps = vec![ScriptStep {
        tool: "script".to_string(),
        args: serde_json::json!({}),
        filter: None,
        context: None,
    }];
    let res = execute_script(&dispatcher, ScriptArgs { steps }, 1024).await.unwrap();
    assert!(res.is_error);
    assert!(res.content.contains("Nested script calls are not allowed"));
}

#[tokio::test]
async fn test_unknown_tool_fails_cleanly() {
    let dispatcher = sample_dispatcher();
    let steps = vec![ScriptStep {
        tool: "ghost_tool".to_string(),
        args: serde_json::json!({}),
        filter: None,
        context: None,
    }];
    let res = execute_script(&dispatcher, ScriptArgs { steps }, 1024).await.unwrap();
    assert!(res.is_error);
    assert!(res.content.contains("Unknown tool: ghost_tool"));
}
