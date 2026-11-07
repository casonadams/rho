use super::*;
use rho_harness_core::model::*;

fn make_tool_turn(cid: &str, tool: &str, cmd: &str, tool_output: &str) -> (ChatMessage, ChatMessage) {
    let call = ToolCall::new(
        cid,
        ToolFunction::new(tool.to_string(), serde_json::json!({ "command": cmd })),
    );
    let res = ToolResult {
        call: cid.to_string(),
        provider: None,
        name: tool.to_string(),
        content: vec![ToolResultContent::Text(TextContent::new(tool_output))],
    };
    (
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(res)],
        },
    )
}

fn make_read_turn(cid: &str, path: &str, tool_output: &str) -> (ChatMessage, ChatMessage) {
    let call = ToolCall::new(
        cid,
        ToolFunction::new("read".to_string(), serde_json::json!({ "path": path })),
    );
    let res = ToolResult {
        call: cid.to_string(),
        provider: None,
        name: "read".to_string(),
        content: vec![ToolResultContent::Text(TextContent::new(tool_output))],
    };
    (
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(res)],
        },
    )
}

fn make_image_read_turn(
    cid: &str,
    path: &str,
    tool_output: &str,
    media_type: Option<&str>,
) -> (ChatMessage, ChatMessage) {
    let call = ToolCall::new(
        cid,
        ToolFunction::new("read".to_string(), serde_json::json!({ "path": path })),
    );
    let res = ToolResult {
        call: cid.to_string(),
        provider: None,
        name: "read".to_string(),
        content: vec![
            ToolResultContent::Text(TextContent::new(tool_output)),
            ToolResultContent::image("iVBORw0KGgoAAAANSUhEUgA=", media_type.map(str::to_string)),
        ],
    };
    (
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(res)],
        },
    )
}

fn make_search_turn(cid: &str, tool: &str, pattern: &str, tool_output: &str) -> (ChatMessage, ChatMessage) {
    let call = ToolCall::new(
        cid,
        ToolFunction::new(tool.to_string(), serde_json::json!({ "pattern": pattern })),
    );
    let res = ToolResult {
        call: cid.to_string(),
        provider: None,
        name: tool.to_string(),
        content: vec![ToolResultContent::Text(TextContent::new(tool_output))],
    };
    (
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(res)],
        },
    )
}

fn make_write_turn(cid: &str, path: &str, content: &str, result_msg: &str) -> (ChatMessage, ChatMessage) {
    let call = ToolCall::new(
        cid,
        ToolFunction::new(
            "write".to_string(),
            serde_json::json!({ "path": path, "content": content }),
        ),
    );
    let res = ToolResult {
        call: cid.to_string(),
        provider: None,
        name: "write".to_string(),
        content: vec![ToolResultContent::Text(TextContent::new(result_msg))],
    };
    (
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(res)],
        },
    )
}

fn make_edit_turn(cid: &str, path: &str, edits: Vec<(&str, &str)>, result_msg: &str) -> (ChatMessage, ChatMessage) {
    let edits_json: Vec<_> = edits
        .into_iter()
        .map(|(old, new)| serde_json::json!({ "oldText": old, "newText": new }))
        .collect();
    let call = ToolCall::new(
        cid,
        ToolFunction::new(
            "edit".to_string(),
            serde_json::json!({ "path": path, "edits": edits_json }),
        ),
    );
    let res = ToolResult {
        call: cid.to_string(),
        provider: None,
        name: "edit".to_string(),
        content: vec![ToolResultContent::Text(TextContent::new(result_msg))],
    };
    (
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(res)],
        },
    )
}

fn make_fetch_turn(cid: &str, url: &str, tool_output: &str) -> (ChatMessage, ChatMessage) {
    let call = ToolCall::new(
        cid,
        ToolFunction::new("web_fetch".to_string(), serde_json::json!({ "url": url })),
    );
    let res = ToolResult {
        call: cid.to_string(),
        provider: None,
        name: "web_fetch".to_string(),
        content: vec![ToolResultContent::Text(TextContent::new(tool_output))],
    };
    (
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(res)],
        },
    )
}

fn make_mcp_turn(cid: &str, tool_name: &str, tool_output: &str) -> (ChatMessage, ChatMessage) {
    let call = ToolCall::new(cid, ToolFunction::new(tool_name.to_string(), serde_json::json!({})));
    let res = ToolResult {
        call: cid.to_string(),
        provider: None,
        name: tool_name.to_string(),
        content: vec![ToolResultContent::Text(TextContent::new(tool_output))],
    };
    (
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call)],
        },
        ChatMessage::User {
            content: vec![UserContent::ToolResult(res)],
        },
    )
}

#[test]
fn test_current_turn_bash_output_is_not_pruned() {
    let output = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let output_with_footer = format!(
        "{output}\n\n[Command completed successfully with exit code 0 (30 lines, 300B). Full log: /tmp/log.txt]"
    );
    let (call_msg, res_msg) = make_tool_turn("c1", "bash", "cargo test", &output_with_footer);

    let history = vec![ChatMessage::user("Please test"), call_msg, res_msg];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    assert_eq!(pruned.len(), 3);

    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    let text = match &res.content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    };
    assert!(text.contains("line 1"));
    assert!(text.contains("line 30"));
}

#[test]
fn test_prior_turn_successful_verbose_bash_output_is_pruned() {
    let output = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let output_with_footer = format!(
        "{output}\n\n[Command completed successfully with exit code 0 (30 lines, 300B). Full log: /tmp/log.txt]"
    );
    let (call_msg, res_msg) = make_tool_turn("c1", "bash", "cargo test", &output_with_footer);

    let history = vec![
        ChatMessage::user("Please test"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Tests passed successfully!"),
        ChatMessage::user("Now run clippy"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    assert_eq!(pruned.len(), 5);

    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    let text = match &res.content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    };
    assert_eq!(
        text,
        "[Command 'cargo test' completed with exit code 0. Output pruned (30 lines, 300B). Full log: /tmp/log.txt]"
    );
}

#[test]
fn test_historical_failed_bash_output_is_pruned() {
    let output = (1..=30).map(|i| format!("error {i}")).collect::<Vec<_>>().join("\n");
    let output_with_err = format!("{output}\n\nCommand exited with code 1");
    let (call_msg, res_msg) = make_tool_turn("c1", "bash", "cargo test", &output_with_err);

    let history = vec![
        ChatMessage::user("Please test"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Tests failed with exit code 1"),
        ChatMessage::user("Fix the tests"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    let text = match &res.content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    };
    assert!(text.contains("[Command 'cargo test' failed with exit code 1 (32 lines,"));
}

#[test]
fn test_recent_failed_bash_output_within_protection_window_is_not_pruned() {
    let output = (1..=30).map(|i| format!("error {i}")).collect::<Vec<_>>().join("\n");
    let output_with_err = format!("{output}\n\nCommand exited with code 1");
    let (call_msg, res_msg) = make_tool_turn("c1", "bash", "cargo test", &output_with_err);

    let history = vec![
        ChatMessage::user("Please test"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Tests failed with exit code 1"),
        ChatMessage::user("Fix the tests"),
    ];

    let policy = PrunePolicy::cache_preserving();
    let pruned = prune_historical_tool_outputs_with_policy(&history, &policy);
    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    let text = match &res.content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    };
    assert!(text.contains("error 1"));
    assert!(text.contains("Command exited with code 1"));
}

#[test]
fn test_historical_failed_bash_output_with_log_path_pruned() {
    let err_body = (1..=30)
        .map(|i| format!("compiler error {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let output = format!(
        "{err_body}\n\n[Showing lines 1-500 of 1200 (50.0KB limit). Full output: /tmp/rho-bash-123.log]\n\nCommand exited with code 1"
    );
    let (call_msg, res_msg) = make_tool_turn("c1", "bash", "cargo build", &output);

    let large_padding = "word ".repeat(50_000);
    let history = vec![
        ChatMessage::user("Build the project"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Build failed with exit code 1"),
        ChatMessage::user(large_padding),
        ChatMessage::assistant("Acknowledge large output"),
        ChatMessage::user("Continue work"),
    ];

    let policy = PrunePolicy {
        prune_minimum_tokens: 0,
        ..PrunePolicy::cache_preserving()
    };
    let pruned = prune_historical_tool_outputs_with_policy(&history, &policy);
    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    let text = match &res.content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    };
    assert!(text.contains("[Command 'cargo build' failed with exit code 1 (1200 lines,"));
    assert!(text.contains("Full log: /tmp/rho-bash-123.log]"));
}

#[test]
fn test_prior_turn_short_bash_output_is_not_pruned() {
    let output = "line 1\nline 2\nline 3\n";
    let (call_msg, res_msg) = make_tool_turn("c1", "bash", "git status", output);

    let history = vec![
        ChatMessage::user("Status"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Clean working tree"),
        ChatMessage::user("Continue"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    let text = match &res.content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    };
    assert_eq!(text, output);
}

#[test]
fn test_prior_turn_verbose_read_output_is_pruned() {
    let output = (1..=40)
        .map(|i| format!("fn line_{i}() {{}}"))
        .collect::<Vec<_>>()
        .join("\n");
    let (call_msg, res_msg) = make_read_turn("c1", "src/lib.rs", &output);

    let history = vec![
        ChatMessage::user("Read file"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Read file done"),
        ChatMessage::user("Next"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    let text = match &res.content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    };
    assert!(text.contains("[File 'src/lib.rs' read (40 lines,"));
    assert!(text.contains("Output pruned for historical turn."));
}

#[test]
fn test_prior_turn_short_read_output_is_not_pruned() {
    let output = "fn small() {}\n";
    let (call_msg, res_msg) = make_read_turn("c1", "src/lib.rs", output);

    let history = vec![
        ChatMessage::user("Read file"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Read file done"),
        ChatMessage::user("Next"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    let text = match &res.content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    };
    assert_eq!(text, output);
}

#[test]
fn test_prior_turn_failed_read_output_is_not_pruned() {
    let output = "File not found: src/missing.rs (in working directory: /tmp)";
    let (call_msg, res_msg) = make_read_turn("c1", "src/missing.rs", output);

    let history = vec![
        ChatMessage::user("Read file"),
        call_msg,
        res_msg,
        ChatMessage::assistant("File is missing"),
        ChatMessage::user("Create it"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    let text = match &res.content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    };
    assert_eq!(text, output);
}

#[test]
fn test_prior_turn_verbose_search_output_is_pruned() {
    let output = (1..=30)
        .map(|i| format!("crates/lib.rs:{i}:match_{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let (call_msg, res_msg) = make_search_turn("c1", "rg", "match_", &output);

    let history = vec![
        ChatMessage::user("Search matches"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Found 30 matches"),
        ChatMessage::user("Continue"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    let text = match &res.content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    };
    assert!(text.contains("[Tool 'rg' with pattern 'match_' completed with 30 lines"));
    assert!(text.contains("Output pruned for historical turn."));
}

#[test]
fn test_prior_turn_failed_search_output_is_not_pruned() {
    let output = "Search timed out after 30s";
    let (call_msg, res_msg) = make_search_turn("c1", "rg", "slow_pattern", output);

    let history = vec![
        ChatMessage::user("Search"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Search timed out"),
        ChatMessage::user("Retry"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    let text = match &res.content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    };
    assert_eq!(text, output);
}

#[test]
fn test_prior_turn_write_tool_call_is_never_pruned() {
    let write_body = (1..=30)
        .map(|i| format!("pub fn generated_func_{i}() {{}}"))
        .collect::<Vec<_>>()
        .join("\n");
    let (call_msg, res_msg) = make_write_turn(
        "c1",
        "src/generated.rs",
        &write_body,
        "Successfully wrote 30 lines to src/generated.rs",
    );

    let history = vec![
        ChatMessage::user("Write generated code"),
        call_msg,
        res_msg,
        ChatMessage::assistant("File written"),
        ChatMessage::user("Now test it"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::Assistant { content, .. } = &pruned[1] else {
        panic!()
    };
    let AssistantContent::ToolCall(call) = &content[0] else {
        panic!()
    };
    let content_val = call.function.arguments.get("content").unwrap().as_str().unwrap();
    assert_eq!(content_val, write_body);
    assert_eq!(
        call.function.arguments.get("path").unwrap().as_str().unwrap(),
        "src/generated.rs"
    );
}

#[test]
fn test_prior_turn_short_write_tool_call_is_not_pruned() {
    let short_body = "pub const VERSION: &str = \"1.0.0\";\n";
    let (call_msg, res_msg) = make_write_turn(
        "c1",
        "src/version.rs",
        short_body,
        "Successfully wrote 1 lines to src/version.rs",
    );

    let history = vec![
        ChatMessage::user("Set version"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Version set"),
        ChatMessage::user("Next"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::Assistant { content, .. } = &pruned[1] else {
        panic!()
    };
    let AssistantContent::ToolCall(call) = &content[0] else {
        panic!()
    };
    let content_val = call.function.arguments.get("content").unwrap().as_str().unwrap();
    assert_eq!(content_val, short_body);
}

#[test]
fn test_current_turn_write_and_read_are_not_pruned() {
    let read_body = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let (read_call, read_res) = make_read_turn("c1", "src/main.rs", &read_body);
    let write_body = (1..=30).map(|i| format!("code {i}")).collect::<Vec<_>>().join("\n");
    let (write_call, write_res) = make_write_turn(
        "c2",
        "src/main.rs",
        &write_body,
        "Successfully wrote 30 lines to src/main.rs",
    );

    let history = vec![
        ChatMessage::user("Active turn work"),
        read_call,
        read_res,
        write_call,
        write_res,
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    assert_eq!(pruned.len(), 5);

    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    let text = match &res.content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    };
    assert_eq!(text, &read_body);

    let ChatMessage::Assistant { content: a_content, .. } = &pruned[3] else {
        panic!()
    };
    let AssistantContent::ToolCall(call) = &a_content[0] else {
        panic!()
    };
    let content_val = call.function.arguments.get("content").unwrap().as_str().unwrap();
    assert_eq!(content_val, &write_body);
}

fn extract_tool_result_first_text(message: &ChatMessage) -> &str {
    let ChatMessage::User { content } = message else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    match &res.content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    }
}

#[test]
fn test_multi_turn_sequence_prunes_completed_verbose_outputs_and_preserves_failures() {
    let out_verbose = (1..=25)
        .map(|i| format!("test {i} passed"))
        .collect::<Vec<_>>()
        .join("\n");
    let out_verbose_footer = format!(
        "{out_verbose}\n\n[Command completed successfully with exit code 0 (25 lines, 400B). Full log: /tmp/test.log]"
    );
    let (call1, res1) = make_tool_turn("c1", "bash", "cargo test", &out_verbose_footer);
    let out_fail = "error[E0425]: cannot find value `x` in this scope\n\nCommand exited with code 1";
    let (call2, res2) = make_tool_turn("c2", "bash", "cargo check", out_fail);
    let (call3, res3) = make_tool_turn("c3", "bash", "cargo build", &out_verbose_footer);

    let t1_history = vec![ChatMessage::user("Run test"), call1.clone(), res1.clone()];
    assert_eq!(
        prune_historical_tool_outputs(&t1_history, 1, DEFAULT_PRUNE_LINE_THRESHOLD),
        t1_history
    );

    let t2_history = vec![
        ChatMessage::user("Run test"),
        call1,
        res1,
        ChatMessage::assistant("Tests passed"),
        ChatMessage::user("Now check"),
        call2.clone(),
        res2.clone(),
    ];
    let t2_pruned = prune_historical_tool_outputs(&t2_history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    assert!(extract_tool_result_first_text(&t2_pruned[2]).contains("Output pruned (25 lines, 400B)"));
    assert!(extract_tool_result_first_text(&t2_pruned[6]).contains("cannot find value `x`"));

    let t3_history = vec![
        t2_pruned[0].clone(),
        t2_pruned[1].clone(),
        t2_pruned[2].clone(),
        t2_pruned[3].clone(),
        t2_pruned[4].clone(),
        call2,
        res2,
        ChatMessage::assistant("Check failed with error E0425"),
        ChatMessage::user("Now build"),
        call3,
        res3,
    ];
    let t3_pruned = prune_historical_tool_outputs(&t3_history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    assert!(extract_tool_result_first_text(&t3_pruned[2]).contains("Output pruned (25 lines, 400B)"));
    assert!(extract_tool_result_first_text(&t3_pruned[6]).contains("cannot find value `x`"));
    assert!(extract_tool_result_first_text(&t3_pruned[10]).contains("test 1 passed"));
}

#[test]
fn test_multi_turn_mixed_tools_sequence_prunes_historical_reads_writes_and_searches() {
    let read_out = (1..=30)
        .map(|i| format!("fn f{i}() {{}}"))
        .collect::<Vec<_>>()
        .join("\n");
    let (read_call, read_res) = make_read_turn("c1", "src/models.rs", &read_out);

    let search_out = (1..=20).map(|i| format!("hit_{i}.rs")).collect::<Vec<_>>().join("\n");
    let (search_call, search_res) = make_search_turn("c2", "fd", "*.rs", &search_out);

    let write_body = (1..=25)
        .map(|i| format!("pub struct S{i};"))
        .collect::<Vec<_>>()
        .join("\n");
    let (write_call, write_res) = make_write_turn("c3", "src/types.rs", &write_body, "Wrote 25 lines");

    let bash_out = (1..=20).map(|i| format!("pass {i}")).collect::<Vec<_>>().join("\n");
    let bash_footer = format!(
        "{bash_out}\n\n[Command completed successfully with exit code 0 (20 lines, 200B). Full log: /tmp/test.log]"
    );
    let (bash_call, bash_res) = make_tool_turn("c4", "bash", "cargo test", &bash_footer);

    let history = vec![
        ChatMessage::user("Inspect codebase"),
        read_call,
        read_res,
        search_call,
        search_res,
        ChatMessage::assistant("Finished inspecting"),
        ChatMessage::user("Implement types and test"),
        write_call,
        write_res,
        bash_call,
        bash_res,
        ChatMessage::assistant("Implementation and tests complete"),
        ChatMessage::user("Now what?"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);

    let read_text = extract_tool_result_first_text(&pruned[2]);
    assert!(read_text.contains("[File 'src/models.rs' read (30 lines,"));

    let search_text = extract_tool_result_first_text(&pruned[4]);
    assert!(search_text.contains("[Tool 'fd' with pattern '*.rs' completed with 20 lines"));

    let ChatMessage::Assistant {
        content: write_content, ..
    } = &pruned[7]
    else {
        panic!()
    };
    let AssistantContent::ToolCall(call) = &write_content[0] else {
        panic!()
    };
    let written_val = call.function.arguments.get("content").unwrap().as_str().unwrap();
    assert_eq!(written_val, write_body);

    let bash_text = extract_tool_result_first_text(&pruned[10]);
    assert!(bash_text.contains("Command 'cargo test' completed with exit code 0. Output pruned"));
}

#[test]
fn test_prior_turn_verbose_web_fetch_output_is_pruned() {
    let output = (1..=30)
        .map(|i| format!("<p>paragraph {i}</p>"))
        .collect::<Vec<_>>()
        .join("\n");
    let (call_msg, res_msg) = make_fetch_turn("c1", "https://example.com/docs", &output);

    let history = vec![
        ChatMessage::user("Fetch docs"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Docs fetched"),
        ChatMessage::user("Next"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let text = extract_tool_result_first_text(&pruned[2]);
    assert!(text.contains("[URL 'https://example.com/docs' fetched (30 lines,"));
    assert!(text.contains("Output pruned for historical turn."));
}

#[test]
fn test_prior_turn_failed_web_fetch_output_is_not_pruned() {
    let output = "Failed to fetch https://example.com: Connection refused";
    let (call_msg, res_msg) = make_fetch_turn("c1", "https://example.com", output);

    let history = vec![
        ChatMessage::user("Fetch"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Failed to fetch"),
        ChatMessage::user("Retry"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let text = extract_tool_result_first_text(&pruned[2]);
    assert_eq!(text, output);
}

#[test]
fn test_prior_turn_verbose_web_search_output_is_pruned() {
    let output = (1..=25)
        .map(|i| format!("{i}. Result title and snippet"))
        .collect::<Vec<_>>()
        .join("\n");
    let (call_msg, res_msg) = make_search_turn("c1", "web_search", "rust tokio", &output);

    let history = vec![
        ChatMessage::user("Search rust"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Search results found"),
        ChatMessage::user("Next"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let text = extract_tool_result_first_text(&pruned[2]);
    assert!(text.contains("[Web search for 'rust tokio' completed with 25 lines"));
}

#[test]
fn test_prior_turn_verbose_mcp_output_is_pruned() {
    let output = (1..=40)
        .map(|i| format!("{{\"record\": {i}}}"))
        .collect::<Vec<_>>()
        .join("\n");
    let (call_msg, res_msg) = make_mcp_turn("c1", "mcp__github__list_issues", &output);

    let history = vec![
        ChatMessage::user("List issues"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Issues listed"),
        ChatMessage::user("Next"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let text = extract_tool_result_first_text(&pruned[2]);
    assert!(text.contains("[Tool 'mcp__github__list_issues' completed with 40 lines"));
}

#[test]
fn test_prior_turn_edit_tool_call_is_never_pruned() {
    let old_text = (1..=30).map(|i| format!("old line {i}")).collect::<Vec<_>>().join("\n");
    let new_text = (1..=30).map(|i| format!("new line {i}")).collect::<Vec<_>>().join("\n");
    let (call_msg, res_msg) = make_edit_turn(
        "c1",
        "src/file.rs",
        vec![(&old_text, &new_text)],
        "Successfully applied 1 replacement(s) to src/file.rs",
    );

    let history = vec![
        ChatMessage::user("Edit file"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Edit applied"),
        ChatMessage::user("Next"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::Assistant { content, .. } = &pruned[1] else {
        panic!()
    };
    let AssistantContent::ToolCall(call) = &content[0] else {
        panic!()
    };
    let edits = call.function.arguments.get("edits").unwrap().as_array().unwrap();
    let old_val = edits[0].get("oldText").unwrap().as_str().unwrap();
    let new_val = edits[0].get("newText").unwrap().as_str().unwrap();
    assert_eq!(old_val, old_text);
    assert_eq!(new_val, new_text);
}

#[test]
fn test_prior_turn_short_edit_tool_call_is_not_pruned() {
    let (call_msg, res_msg) = make_edit_turn(
        "c1",
        "src/file.rs",
        vec![("let x = 1;", "let x = 2;")],
        "Successfully applied 1 replacement(s) to src/file.rs",
    );

    let history = vec![
        ChatMessage::user("Edit file"),
        call_msg,
        res_msg,
        ChatMessage::assistant("Edit applied"),
        ChatMessage::user("Next"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::Assistant { content, .. } = &pruned[1] else {
        panic!()
    };
    let AssistantContent::ToolCall(call) = &content[0] else {
        panic!()
    };
    let edits = call.function.arguments.get("edits").unwrap().as_array().unwrap();
    assert_eq!(edits[0].get("oldText").unwrap().as_str().unwrap(), "let x = 1;");
    assert_eq!(edits[0].get("newText").unwrap().as_str().unwrap(), "let x = 2;");
}

#[test]
fn test_prune_image_tool_result_in_historical_turn() {
    let (read_call, read_res) = make_image_read_turn(
        "c1",
        "screenshot.png",
        "Read image file [image/png]\n[Image: 800x600]",
        Some("image/png"),
    );

    let history = vec![
        ChatMessage::user("Inspect screenshot"),
        read_call,
        read_res,
        ChatMessage::assistant("The screenshot shows a login form."),
        ChatMessage::user("Now fix the login form"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };

    assert_eq!(res.content.len(), 1);
    let ToolResultContent::Text(text) = &res.content[0] else {
        panic!("Expected text block")
    };
    assert_eq!(
        text.text,
        "[Image 'screenshot.png' (image/png) read. Image content pruned for historical turn.]"
    );
}

#[test]
fn test_preserve_image_tool_result_in_volatile_turn() {
    let (read_call, read_res) = make_image_read_turn(
        "c1",
        "screenshot.png",
        "Read image file [image/png]\n[Image: 800x600]",
        Some("image/png"),
    );

    let history = vec![ChatMessage::user("Inspect screenshot"), read_call, read_res];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };

    assert_eq!(res.content.len(), 2);
    assert!(matches!(res.content[0], ToolResultContent::Text(_)));
    assert!(matches!(res.content[1], ToolResultContent::Image(_)));
}

#[test]
fn test_image_stub_formatting_variants() {
    let (call1, res1) = make_image_read_turn(
        "c1",
        "assets/logo.png",
        "Read image file [image/png]",
        Some("image/png"),
    );
    let (call2, res2) = make_image_read_turn("c2", "assets/photo.jpg", "Read image file [image/jpeg]", None);

    let custom_call = ToolCall::new(
        "c3",
        ToolFunction::new("screenshot".to_string(), serde_json::json!({ "target": "window" })),
    );
    let custom_res = ToolResult {
        call: "c3".to_string(),
        provider: None,
        name: "screenshot".to_string(),
        content: vec![
            ToolResultContent::Text(TextContent::new("Captured screenshot")),
            ToolResultContent::image("iVBORw0KGgoAAAANSUhEUgA=", Some("image/png".to_string())),
        ],
    };
    let custom_call_msg = ChatMessage::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(custom_call)],
    };
    let custom_res_msg = ChatMessage::User {
        content: vec![UserContent::ToolResult(custom_res)],
    };

    let history = vec![
        ChatMessage::user("Inspect images"),
        call1,
        res1,
        call2,
        res2,
        custom_call_msg,
        custom_res_msg,
        ChatMessage::assistant("All images inspected"),
        ChatMessage::user("Next step"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);

    let extract_text = |idx: usize| -> String {
        let ChatMessage::User { content } = &pruned[idx] else {
            panic!()
        };
        let UserContent::ToolResult(res) = &content[0] else {
            panic!()
        };
        match &res.content[0] {
            ToolResultContent::Text(t) => t.text.clone(),
            _ => panic!(),
        }
    };

    assert_eq!(
        extract_text(2),
        "[Image 'assets/logo.png' (image/png) read. Image content pruned for historical turn.]"
    );
    assert_eq!(
        extract_text(4),
        "[Image 'assets/photo.jpg' (image/jpeg) read. Image content pruned for historical turn.]"
    );
    assert_eq!(
        extract_text(6),
        "[Tool 'screenshot' for 'window' returned image (image/png). Image content pruned for historical turn.]"
    );
}

#[test]
fn test_protect_recent_tokens_prevents_pruning() {
    let (call, res) = make_tool_turn("c1", "bash", "cargo test", &"verbose test output\n".repeat(30));
    let history = vec![
        ChatMessage::user("Run test"),
        call,
        res,
        ChatMessage::assistant("Done test"),
        ChatMessage::user("Next step"),
    ];

    let policy_protected = PrunePolicy {
        volatile_turns: 1,
        line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
        protect_recent_tokens: 30_000,
        prune_minimum_tokens: 0,
        min_prune_tokens: 0,
        model: None,
    };
    let pruned = prune_historical_tool_outputs_with_policy(&history, &policy_protected);
    assert_eq!(pruned, history);

    let policy_unprotected = PrunePolicy {
        volatile_turns: 1,
        line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
        protect_recent_tokens: 5,
        prune_minimum_tokens: 0,
        min_prune_tokens: 0,
        model: None,
    };
    let pruned_unprotected = prune_historical_tool_outputs_with_policy(&history, &policy_unprotected);
    assert_ne!(pruned_unprotected, history);
    let ChatMessage::User { content } = &pruned_unprotected[2] else {
        panic!()
    };
    let UserContent::ToolResult(r) = &content[0] else {
        panic!()
    };
    let ToolResultContent::Text(t) = &r.content[0] else {
        panic!()
    };
    assert!(t.text.contains("completed with exit code 0"));
}

#[test]
fn test_prune_minimum_tokens_savings_floor() {
    let (call, res) = make_tool_turn("c1", "bash", "cargo test", &"verbose test output\n".repeat(30));
    let history = vec![
        ChatMessage::user("Run test"),
        call,
        res,
        ChatMessage::assistant("Done test"),
        ChatMessage::user("Next step"),
    ];

    let high_savings_floor = PrunePolicy {
        volatile_turns: 1,
        line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
        protect_recent_tokens: 0,
        prune_minimum_tokens: 50_000,
        min_prune_tokens: 0,
        model: None,
    };
    let pruned_skipped = prune_historical_tool_outputs_with_policy(&history, &high_savings_floor);
    assert_eq!(pruned_skipped, history);

    let low_savings_floor = PrunePolicy {
        volatile_turns: 1,
        line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
        protect_recent_tokens: 0,
        prune_minimum_tokens: 10,
        min_prune_tokens: 0,
        model: None,
    };
    let pruned_executed = prune_historical_tool_outputs_with_policy(&history, &low_savings_floor);
    assert_ne!(pruned_executed, history);
}

#[test]
fn test_min_prune_tokens_skips_small_outputs() {
    let short_lines = "x\n".repeat(25);
    let (call, res) = make_tool_turn("c1", "bash", "echo", &short_lines);
    let history = vec![
        ChatMessage::user("Run echo"),
        call,
        res,
        ChatMessage::assistant("Done echo"),
        ChatMessage::user("Next step"),
    ];

    let policy_skip_small = PrunePolicy {
        volatile_turns: 1,
        line_threshold: DEFAULT_PRUNE_LINE_THRESHOLD,
        protect_recent_tokens: 0,
        prune_minimum_tokens: 0,
        min_prune_tokens: 100,
        model: None,
    };
    let pruned = prune_historical_tool_outputs_with_policy(&history, &policy_skip_small);
    assert_eq!(pruned, history);
}

#[test]
fn test_prune_policy_for_context_window() {
    let p_128k = PrunePolicy::for_context_window(128_000);
    assert_eq!(p_128k.prune_minimum_tokens, 6_400);
    assert_eq!(p_128k.protect_recent_tokens, 19_200);

    let p_200k = PrunePolicy::for_context_window(200_000);
    assert_eq!(p_200k.prune_minimum_tokens, 10_000);
    assert_eq!(p_200k.protect_recent_tokens, 30_000);

    let p_1m = PrunePolicy::for_context_window(1_000_000);
    assert_eq!(p_1m.prune_minimum_tokens, 20_000);
    assert_eq!(p_1m.protect_recent_tokens, 30_000);

    let p_tiny = PrunePolicy::for_context_window(32_000);
    assert_eq!(p_tiny.prune_minimum_tokens, 3_000);
    assert_eq!(p_tiny.protect_recent_tokens, 10_000);
}

#[test]
fn test_adaptive_pruning_floor_triggers_on_smaller_window() {
    let body = "verbose compiler warning line output\n".repeat(1_600);
    let (call, res) = make_tool_turn("c1", "bash", "cargo build", &body);
    let history = vec![
        ChatMessage::user("Build"),
        call,
        res,
        ChatMessage::assistant("Done build"),
        ChatMessage::user("Next command"),
    ];

    let mut static_policy = PrunePolicy::cache_preserving();
    static_policy.protect_recent_tokens = 0;
    let unpruned = prune_historical_tool_outputs_with_policy(&history, &static_policy);
    assert_eq!(unpruned, history);

    let mut adaptive_policy = PrunePolicy::for_context_window(128_000);
    adaptive_policy.protect_recent_tokens = 0;
    let pruned = prune_historical_tool_outputs_with_policy(&history, &adaptive_policy);
    assert_ne!(pruned, history);
}

#[test]
fn test_prune_policy_model_propagation() {
    let policy = PrunePolicy::cache_preserving().with_model("claude-3-7-sonnet");
    assert_eq!(policy.model.as_deref(), Some("claude-3-7-sonnet"));
}

#[test]
fn test_skill_tool_output_never_pruned() {
    let skill_output = "Skill instructions line\n".repeat(50);
    let (call, res) = make_tool_turn("c1", "skill", "coding-standard", &skill_output);
    let history = vec![
        ChatMessage::user("Load skill"),
        call,
        res,
        ChatMessage::assistant("Skill loaded"),
        ChatMessage::user("Next step"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    assert_eq!(pruned, history);
}

#[test]
fn test_superseded_file_reads_detected_and_elided() {
    let (read_call1, read_res1) = make_read_turn("c1", "src/lib.rs", &"pub fn foo() {}\n".repeat(20));
    let edit_call = ToolCall::new(
        "c2",
        ToolFunction::new(
            "edit".to_string(),
            serde_json::json!({ "path": "src/lib.rs", "content": "edited" }),
        ),
    );
    let edit_res = ToolResult {
        call: "c2".to_string(),
        provider: None,
        name: "edit".to_string(),
        content: vec![ToolResultContent::Text(TextContent::new("Successfully replaced"))],
    };
    let edit_call_msg = ChatMessage::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(edit_call)],
    };
    let edit_res_msg = ChatMessage::User {
        content: vec![UserContent::ToolResult(edit_res)],
    };

    let history = vec![
        ChatMessage::user("Read file"),
        read_call1,
        read_res1,
        ChatMessage::assistant("I read lib.rs"),
        ChatMessage::user("Now edit it"),
        edit_call_msg,
        edit_res_msg,
        ChatMessage::assistant("File edited"),
        ChatMessage::user("Next step"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);
    let ChatMessage::User { content } = &pruned[2] else {
        panic!()
    };
    let UserContent::ToolResult(r) = &content[0] else {
        panic!()
    };
    let ToolResultContent::Text(t) = &r.content[0] else {
        panic!()
    };
    assert_eq!(t.text, "[File read superseded by a later operation on src/lib.rs]");
}

fn call_message(cid: &str, name: &str, args: serde_json::Value) -> ChatMessage {
    ChatMessage::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall::new(
            cid,
            ToolFunction::new(name.to_string(), args),
        ))],
    }
}

#[test]
fn test_partial_read_does_not_supersede_earlier_full_read() {
    let messages = vec![
        call_message("c1", "read", serde_json::json!({ "path": "src/lib.rs" })),
        ChatMessage::user("Next"),
        call_message(
            "c2",
            "read",
            serde_json::json!({ "path": "src/lib.rs", "offset": 10, "limit": 5 }),
        ),
    ];

    assert!(collect_superseded_tool_call_ids(&messages).is_empty());
}

#[test]
fn test_full_read_supersedes_earlier_partial_read() {
    let messages = vec![
        call_message(
            "c1",
            "read",
            serde_json::json!({ "path": "src/lib.rs", "offset": 10, "limit": 5 }),
        ),
        call_message("c2", "read", serde_json::json!({ "path": "src/lib.rs" })),
    ];

    assert_eq!(
        collect_superseded_tool_call_ids(&messages),
        HashSet::from(["c1".to_string()])
    );
}

#[test]
fn test_tool_name_matching_ignores_case_when_superseding_reads() {
    let messages = vec![
        call_message("c1", "read", serde_json::json!({ "path": "src/lib.rs" })),
        call_message("c2", "Edit", serde_json::json!({ "path": "src/lib.rs" })),
    ];

    assert_eq!(
        collect_superseded_tool_call_ids(&messages),
        HashSet::from(["c1".to_string()])
    );
}

#[test]
fn test_source_text_mentioning_read_errors_is_still_pruned() {
    let source = "// Failed to read the config\n".repeat(40);
    let (call, result) = make_read_turn("c1", "src/errors.rs", &source);
    let history = vec![
        ChatMessage::user("Read file"),
        call,
        result,
        ChatMessage::assistant("Read done"),
        ChatMessage::user("Next"),
    ];

    let pruned = prune_historical_tool_outputs(&history, 1, DEFAULT_PRUNE_LINE_THRESHOLD);

    assert!(extract_tool_result_first_text(&pruned[2]).contains("Output pruned for historical turn."));
}
