use super::*;
use crate::engine::metrics::{PruneAction, PruneDecision};
use crate::engine::runner::turn::prune::cache::WARM_REBUILD_RATIO;
use rho_harness_core::model::*;
use rho_harness_core::tokens::MessageTokenCache;
use test_case::test_case;

fn unthrottled_policy() -> PrunePolicy {
    PrunePolicy {
        protect_recent_tokens: 0,
        min_total_savings: 0,
        min_result_tokens: 0,
        ..PrunePolicy::cache_preserving()
    }
}

fn prune_historical_tool_outputs(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    prune_historical_tool_outputs_with_policy(messages, &unthrottled_policy())
}

fn tool_turn(
    cid: &str,
    name: &str,
    args: serde_json::Value,
    content: Vec<ToolResultContent>,
) -> (ChatMessage, ChatMessage) {
    let call = ToolCall::new(cid, ToolFunction::new(name.to_string(), args));
    let res = ToolResult {
        call: cid.to_string(),
        provider: None,
        name: name.to_string(),
        content,
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

fn text_turn(cid: &str, name: &str, args: serde_json::Value, tool_output: &str) -> (ChatMessage, ChatMessage) {
    tool_turn(
        cid,
        name,
        args,
        vec![ToolResultContent::Text(TextContent::new(tool_output))],
    )
}

fn make_tool_turn(cid: &str, tool: &str, cmd: &str, tool_output: &str) -> (ChatMessage, ChatMessage) {
    text_turn(cid, tool, serde_json::json!({ "command": cmd }), tool_output)
}

fn make_read_turn(cid: &str, path: &str, tool_output: &str) -> (ChatMessage, ChatMessage) {
    text_turn(cid, "read", serde_json::json!({ "path": path }), tool_output)
}

fn make_image_read_turn(
    cid: &str,
    path: &str,
    tool_output: &str,
    media_type: Option<&str>,
) -> (ChatMessage, ChatMessage) {
    tool_turn(
        cid,
        "read",
        serde_json::json!({ "path": path }),
        vec![
            ToolResultContent::Text(TextContent::new(tool_output)),
            ToolResultContent::image("iVBORw0KGgoAAAANSUhEUgA=", media_type.map(str::to_string)),
        ],
    )
}

fn make_search_turn(cid: &str, tool: &str, pattern: &str, tool_output: &str) -> (ChatMessage, ChatMessage) {
    text_turn(cid, tool, serde_json::json!({ "pattern": pattern }), tool_output)
}

fn make_write_turn(cid: &str, path: &str, content: &str, result_msg: &str) -> (ChatMessage, ChatMessage) {
    text_turn(
        cid,
        "write",
        serde_json::json!({ "path": path, "content": content }),
        result_msg,
    )
}

fn make_edit_turn(cid: &str, path: &str, edits: Vec<(&str, &str)>, result_msg: &str) -> (ChatMessage, ChatMessage) {
    let edits_json: Vec<_> = edits
        .into_iter()
        .map(|(old, new)| serde_json::json!({ "oldText": old, "newText": new }))
        .collect();
    text_turn(
        cid,
        "edit",
        serde_json::json!({ "path": path, "edits": edits_json }),
        result_msg,
    )
}

fn make_fetch_turn(cid: &str, url: &str, tool_output: &str) -> (ChatMessage, ChatMessage) {
    text_turn(cid, "web_fetch", serde_json::json!({ "url": url }), tool_output)
}

fn make_mcp_turn(cid: &str, tool_name: &str, tool_output: &str) -> (ChatMessage, ChatMessage) {
    text_turn(cid, tool_name, serde_json::json!({}), tool_output)
}

fn prior_turn_history(request: &str, turn: (ChatMessage, ChatMessage), reply: &str, next: &str) -> Vec<ChatMessage> {
    let (call, result) = turn;
    vec![
        ChatMessage::user(request),
        call,
        result,
        ChatMessage::assistant(reply),
        ChatMessage::user(next),
    ]
}

fn prune_prior_turn_text(request: &str, turn: (ChatMessage, ChatMessage), reply: &str, next: &str) -> String {
    let pruned = prune_historical_tool_outputs(&prior_turn_history(request, turn, reply, next));
    extract_tool_result_first_text(&pruned[2]).to_string()
}

fn tool_result(message: &ChatMessage) -> &ToolResult {
    let ChatMessage::User { content } = message else {
        panic!()
    };
    let UserContent::ToolResult(res) = &content[0] else {
        panic!()
    };
    res
}

fn extract_tool_result_first_text(message: &ChatMessage) -> &str {
    match &tool_result(message).content[0] {
        ToolResultContent::Text(t) => &t.text,
        _ => panic!(),
    }
}

fn tool_call_arguments(message: &ChatMessage) -> &serde_json::Value {
    let ChatMessage::Assistant { content, .. } = message else {
        panic!()
    };
    let AssistantContent::ToolCall(call) = &content[0] else {
        panic!()
    };
    &call.function.arguments
}

#[test]
fn test_current_turn_bash_output_is_not_pruned() {
    let output = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let output_with_footer = format!(
        "{output}\n\n[Command completed successfully with exit code 0 (30 lines, 300B). Full log: /tmp/log.txt]"
    );
    let (call_msg, res_msg) = make_tool_turn("c1", "bash", "cargo test", &output_with_footer);

    let history = vec![ChatMessage::user("Please test"), call_msg, res_msg];

    let pruned = prune_historical_tool_outputs(&history);
    assert_eq!(pruned.len(), 3);

    let text = extract_tool_result_first_text(&pruned[2]);
    assert!(text.contains("line 1"));
    assert!(text.contains("line 30"));
}

#[test]
fn test_prior_turn_successful_verbose_bash_output_is_pruned() {
    let output = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let output_with_footer = format!(
        "{output}\n\n[Command completed successfully with exit code 0 (30 lines, 300B). Full log: /tmp/log.txt]"
    );
    let history = prior_turn_history(
        "Please test",
        make_tool_turn("c1", "bash", "cargo test", &output_with_footer),
        "Tests passed successfully!",
        "Now run clippy",
    );

    let pruned = prune_historical_tool_outputs(&history);
    assert_eq!(pruned.len(), 5);
    assert_eq!(
        extract_tool_result_first_text(&pruned[2]),
        "[bash 'cargo test': exit 0, pruned 30 lines, 300B; log: /tmp/log.txt]"
    );
}

#[test_case(0, "exit 0"; "success")]
#[test_case(101, "exit 101"; "failure")]
fn pruning_recognises_the_footer_the_bash_tool_writes(exit_code: i32, status: &str) {
    let footer = crate::tools::bash::footer::BashFooter {
        exit_code,
        lines: 30,
        size: "300B".to_string(),
        log_path: Some("/tmp/rho-bash/ab12cd34ef56.log".to_string()),
    };
    assert_footer_prunes_to(
        &footer,
        &format!("[bash 'cargo test': {status}, pruned 30 lines, 300B; log: /tmp/rho-bash/ab12cd34ef56.log]"),
    );
}

fn assert_footer_prunes_to(footer: &crate::tools::bash::footer::BashFooter, stub: &str) {
    let output = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let summary = "[1 failing test; details from log]\n- math::subtracts\n    assertion failed";
    for body in [output.clone(), format!("{output}\n\n{summary}")] {
        let turn = make_tool_turn("c1", "bash", "cargo test", &format!("{body}\n\n{}", footer.render()));
        assert_eq!(prune_prior_turn_text("Please test", turn, "Done", "Next"), stub);
    }
}

#[test]
fn test_historical_failed_bash_output_is_pruned() {
    let output = (1..=30).map(|i| format!("error {i}")).collect::<Vec<_>>().join("\n");
    let output_with_err = format!("{output}\n\nCommand exited with code 1");
    let text = prune_prior_turn_text(
        "Please test",
        make_tool_turn("c1", "bash", "cargo test", &output_with_err),
        "Tests failed with exit code 1",
        "Fix the tests",
    );
    assert!(text.contains("[bash 'cargo test': exit 1, pruned 32 lines,"));
}

#[test]
fn test_recent_failed_bash_output_within_protection_window_is_not_pruned() {
    let output = (1..=30).map(|i| format!("error {i}")).collect::<Vec<_>>().join("\n");
    let output_with_err = format!("{output}\n\nCommand exited with code 1");
    let history = prior_turn_history(
        "Please test",
        make_tool_turn("c1", "bash", "cargo test", &output_with_err),
        "Tests failed with exit code 1",
        "Fix the tests",
    );

    let pruned = prune_historical_tool_outputs_with_policy(&history, &PrunePolicy::cache_preserving());
    let text = extract_tool_result_first_text(&pruned[2]);
    assert!(text.contains("error 1"));
    assert!(text.contains("Command exited with code 1"));
}

#[test_case("[lines 1-500 of 1200 (50.0KB limit); log: /tmp/rho-bash-123.log]\n\n[exit 1]"; "log footer")]
#[test_case(
    "[Showing lines 1-500 of 1200 (50.0KB limit). Full output: /tmp/rho-bash-123.log]\n\nCommand exited with code 1";
    "legacy full output footer"
)]
#[test_case(
    "[lines 1-500 of 1200 (50.0KB limit); log: /tmp/rho-bash-123.log]\n\n\
     [1 failing test; details from log]\n- math::subtracts\n    assertion failed\n\n[exit 1]";
    "log footer with failure summary"
)]
fn test_historical_failed_bash_output_with_log_path_pruned(footer: &str) {
    let err_body = (1..=30)
        .map(|i| format!("compiler error {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_failed_bash_stub(&format!("{err_body}\n\n{footer}"));
}

fn assert_failed_bash_stub(output: &str) {
    let mut history = prior_turn_history(
        "Build the project",
        make_tool_turn("c1", "bash", "cargo build", output),
        "Build failed with exit code 1",
        &"word ".repeat(50_000),
    );
    history.extend([
        ChatMessage::assistant("Acknowledge large output"),
        ChatMessage::user("Continue work"),
    ]);

    let policy = PrunePolicy {
        min_total_savings: 0,
        ..PrunePolicy::cache_preserving()
    };
    let pruned = prune_historical_tool_outputs_with_policy(&history, &policy);
    let text = extract_tool_result_first_text(&pruned[2]);
    assert!(text.contains("[bash 'cargo build': exit 1, pruned 1200 lines,"));
    assert!(text.contains("; log: /tmp/rho-bash-123.log]"));
}

const SHORT_BASH: &str = "line 1\nline 2\nline 3\n";
const FAILED_SEARCH: &str = "Search timed out after 30s";

#[test_case(
    "Status",
    make_tool_turn("c1", "bash", "git status", SHORT_BASH),
    SHORT_BASH,
    "Clean working tree",
    "Continue";
    "short bash"
)]
#[test_case(
    "Search",
    make_search_turn("c1", "rg", "slow_pattern", FAILED_SEARCH),
    FAILED_SEARCH,
    "Search timed out",
    "Retry";
    "failed search"
)]
fn test_prior_turn_short_bash_and_failed_search_outputs_are_not_pruned(
    request: &str,
    turn: (ChatMessage, ChatMessage),
    output: &str,
    reply: &str,
    next: &str,
) {
    assert_eq!(prune_prior_turn_text(request, turn, reply, next), output);
}

#[test]
fn test_prior_turn_verbose_read_output_is_pruned() {
    let output = (1..=40)
        .map(|i| format!("fn line_{i}() {{}}"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = prune_prior_turn_text(
        "Read file",
        make_read_turn("c1", "src/lib.rs", &output),
        "Read file done",
        "Next",
    );
    assert!(text.contains("[read 'src/lib.rs': pruned 40 lines,"));
}

const SHORT_READ: &str = "fn small() {}\n";
const FAILED_READ: &str = "File not found: src/missing.rs (in working directory: /tmp)";

#[test_case("src/lib.rs", SHORT_READ, "Read file done", "Next"; "short read")]
#[test_case("src/missing.rs", FAILED_READ, "File is missing", "Create it"; "failed read")]
fn test_prior_turn_short_and_failed_read_outputs_are_not_pruned(path: &str, output: &str, reply: &str, next: &str) {
    let turn = make_read_turn("c1", path, output);
    assert_eq!(prune_prior_turn_text("Read file", turn, reply, next), output);
}

#[test]
fn test_prior_turn_verbose_search_output_is_pruned() {
    let output = (1..=30)
        .map(|i| format!("crates/lib.rs:{i}:match_{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = prune_prior_turn_text(
        "Search matches",
        make_search_turn("c1", "rg", "match_", &output),
        "Found 30 matches",
        "Continue",
    );
    assert!(text.contains("[rg 'match_': pruned 30 lines,"));
}

#[test]
fn test_prior_turn_write_tool_call_is_never_pruned() {
    let write_body = (1..=30)
        .map(|i| format!("pub fn generated_func_{i}() {{}}"))
        .collect::<Vec<_>>()
        .join("\n");
    let history = prior_turn_history(
        "Write generated code",
        make_write_turn(
            "c1",
            "src/generated.rs",
            &write_body,
            "Successfully wrote 30 lines to src/generated.rs",
        ),
        "File written",
        "Now test it",
    );

    let pruned = prune_historical_tool_outputs(&history);
    let args = tool_call_arguments(&pruned[1]);
    assert_eq!(args.get("content").unwrap().as_str().unwrap(), write_body);
    assert_eq!(args.get("path").unwrap().as_str().unwrap(), "src/generated.rs");
}

#[test]
fn test_prior_turn_short_write_tool_call_is_not_pruned() {
    let short_body = "pub const VERSION: &str = \"1.0.0\";\n";
    let history = prior_turn_history(
        "Set version",
        make_write_turn(
            "c1",
            "src/version.rs",
            short_body,
            "Successfully wrote 1 lines to src/version.rs",
        ),
        "Version set",
        "Next",
    );

    let pruned = prune_historical_tool_outputs(&history);
    let content_val = tool_call_arguments(&pruned[1])
        .get("content")
        .unwrap()
        .as_str()
        .unwrap();
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

    let pruned = prune_historical_tool_outputs(&history);
    assert_eq!(pruned.len(), 5);
    assert_eq!(extract_tool_result_first_text(&pruned[2]), read_body);
    let content_val = tool_call_arguments(&pruned[3])
        .get("content")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(content_val, write_body);
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
    assert_eq!(prune_historical_tool_outputs(&t1_history), t1_history);

    let t2_history = vec![
        ChatMessage::user("Run test"),
        call1,
        res1,
        ChatMessage::assistant("Tests passed"),
        ChatMessage::user("Now check"),
        call2.clone(),
        res2.clone(),
    ];
    let t2_pruned = prune_historical_tool_outputs(&t2_history);
    assert!(extract_tool_result_first_text(&t2_pruned[2]).contains("exit 0, pruned 25 lines, 400B"));
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
    let t3_pruned = prune_historical_tool_outputs(&t3_history);
    assert!(extract_tool_result_first_text(&t3_pruned[2]).contains("exit 0, pruned 25 lines, 400B"));
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

    let pruned = prune_historical_tool_outputs(&history);

    let read_text = extract_tool_result_first_text(&pruned[2]);
    assert!(read_text.contains("[read 'src/models.rs': pruned 30 lines,"));

    let search_text = extract_tool_result_first_text(&pruned[4]);
    assert!(search_text.contains("[fd '*.rs': pruned 20 lines,"));

    let written_val = tool_call_arguments(&pruned[7])
        .get("content")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(written_val, write_body);

    let bash_text = extract_tool_result_first_text(&pruned[10]);
    assert!(bash_text.contains("[bash 'cargo test': exit 0, pruned"));
}

#[test]
fn test_prior_turn_verbose_web_fetch_and_mcp_outputs_are_pruned() {
    let fetch_output = (1..=30)
        .map(|i| format!("<p>paragraph {i}</p>"))
        .collect::<Vec<_>>()
        .join("\n");
    let mcp_output = (1..=40)
        .map(|i| format!("{{\"record\": {i}}}"))
        .collect::<Vec<_>>()
        .join("\n");
    let cases = [
        (
            "verbose web_fetch",
            "Fetch docs",
            make_fetch_turn("c1", "https://example.com/docs", &fetch_output),
            "Docs fetched",
            "[web_fetch 'https://example.com/docs': pruned 30 lines,",
        ),
        (
            "verbose mcp",
            "List issues",
            make_mcp_turn("c1", "mcp__github__list_issues", &mcp_output),
            "Issues listed",
            "[mcp__github__list_issues: pruned 40 lines,",
        ),
    ];
    for (case, request, turn, reply, stub) in cases {
        let text = prune_prior_turn_text(request, turn, reply, "Next");
        assert!(text.contains(stub), "{case}: {text}");
    }
}

#[test]
fn test_prior_turn_failed_web_fetch_output_is_not_pruned() {
    let output = "Failed to fetch https://example.com: Connection refused";
    let text = prune_prior_turn_text(
        "Fetch",
        make_fetch_turn("c1", "https://example.com", output),
        "Failed to fetch",
        "Retry",
    );
    assert_eq!(text, output);
}

#[test]
fn test_prior_turn_verbose_web_search_output_is_pruned() {
    let output = (1..=25)
        .map(|i| format!("{i}. Result title and snippet"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = prune_prior_turn_text(
        "Search rust",
        make_search_turn("c1", "web_search", "rust tokio", &output),
        "Search results found",
        "Next",
    );
    assert!(text.contains("[web_search 'rust tokio': pruned 25 lines,"));
}

#[test]
fn test_prior_turn_edit_tool_call_is_never_pruned() {
    let old_text = (1..=30).map(|i| format!("old line {i}")).collect::<Vec<_>>().join("\n");
    let new_text = (1..=30).map(|i| format!("new line {i}")).collect::<Vec<_>>().join("\n");
    let history = prior_turn_history(
        "Edit file",
        make_edit_turn(
            "c1",
            "src/file.rs",
            vec![(&old_text, &new_text)],
            "Successfully applied 1 replacement(s) to src/file.rs",
        ),
        "Edit applied",
        "Next",
    );

    let pruned = prune_historical_tool_outputs(&history);
    let edits = tool_call_arguments(&pruned[1])
        .get("edits")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(edits[0].get("oldText").unwrap().as_str().unwrap(), old_text);
    assert_eq!(edits[0].get("newText").unwrap().as_str().unwrap(), new_text);
}

#[test]
fn test_prior_turn_short_edit_tool_call_is_not_pruned() {
    let history = prior_turn_history(
        "Edit file",
        make_edit_turn(
            "c1",
            "src/file.rs",
            vec![("let x = 1;", "let x = 2;")],
            "Successfully applied 1 replacement(s) to src/file.rs",
        ),
        "Edit applied",
        "Next",
    );

    let pruned = prune_historical_tool_outputs(&history);
    let edits = tool_call_arguments(&pruned[1])
        .get("edits")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(edits[0].get("oldText").unwrap().as_str().unwrap(), "let x = 1;");
    assert_eq!(edits[0].get("newText").unwrap().as_str().unwrap(), "let x = 2;");
}

#[test]
fn test_prune_image_tool_result_in_historical_turn() {
    let history = prior_turn_history(
        "Inspect screenshot",
        make_image_read_turn(
            "c1",
            "screenshot.png",
            "Read image file [image/png]\n[Image: 800x600]",
            Some("image/png"),
        ),
        "The screenshot shows a login form.",
        "Now fix the login form",
    );

    let pruned = prune_historical_tool_outputs(&history);
    assert_eq!(tool_result(&pruned[2]).content.len(), 1);
    assert_eq!(
        extract_tool_result_first_text(&pruned[2]),
        "[read 'screenshot.png': image (image/png), pruned]"
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

    let pruned = prune_historical_tool_outputs(&history);
    let res = tool_result(&pruned[2]);
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
    let (custom_call_msg, custom_res_msg) = tool_turn(
        "c3",
        "screenshot",
        serde_json::json!({ "target": "window" }),
        vec![
            ToolResultContent::Text(TextContent::new("Captured screenshot")),
            ToolResultContent::image("iVBORw0KGgoAAAANSUhEUgA=", Some("image/png".to_string())),
        ],
    );

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

    let pruned = prune_historical_tool_outputs(&history);
    assert_eq!(
        extract_tool_result_first_text(&pruned[2]),
        "[read 'assets/logo.png': image (image/png), pruned]"
    );
    assert_eq!(
        extract_tool_result_first_text(&pruned[4]),
        "[read 'assets/photo.jpg': image (image/jpeg), pruned]"
    );
    assert_eq!(
        extract_tool_result_first_text(&pruned[6]),
        "[screenshot 'window': image (image/png), pruned]"
    );
}

#[test]
fn test_protect_recent_tokens_prevents_pruning() {
    let history = prior_turn_history(
        "Run test",
        make_tool_turn("c1", "bash", "cargo test", &"verbose test output\n".repeat(30)),
        "Done test",
        "Next step",
    );

    let policy_protected = PrunePolicy {
        protect_recent_tokens: 30_000,
        ..unthrottled_policy()
    };
    let pruned = prune_historical_tool_outputs_with_policy(&history, &policy_protected);
    assert_eq!(pruned, history);

    let policy_unprotected = PrunePolicy {
        protect_recent_tokens: 5,
        ..unthrottled_policy()
    };
    let pruned_unprotected = prune_historical_tool_outputs_with_policy(&history, &policy_unprotected);
    assert_ne!(pruned_unprotected, history);
    assert!(extract_tool_result_first_text(&pruned_unprotected[2]).contains("exit 0, pruned"));
}

#[test]
fn test_min_total_savings_floor() {
    let history = prior_turn_history(
        "Run test",
        make_tool_turn("c1", "bash", "cargo test", &"verbose test output\n".repeat(30)),
        "Done test",
        "Next step",
    );

    let high_savings_floor = PrunePolicy {
        min_total_savings: 50_000,
        ..unthrottled_policy()
    };
    let pruned_skipped = prune_historical_tool_outputs_with_policy(&history, &high_savings_floor);
    assert_eq!(pruned_skipped, history);

    let low_savings_floor = PrunePolicy {
        min_total_savings: 10,
        ..unthrottled_policy()
    };
    let pruned_executed = prune_historical_tool_outputs_with_policy(&history, &low_savings_floor);
    assert_ne!(pruned_executed, history);
}

#[test]
fn test_min_result_tokens_skips_small_outputs() {
    let history = prior_turn_history(
        "Run echo",
        make_tool_turn("c1", "bash", "echo", &"x\n".repeat(25)),
        "Done echo",
        "Next step",
    );

    let policy_skip_small = PrunePolicy {
        min_result_tokens: 100,
        ..unthrottled_policy()
    };
    let pruned = prune_historical_tool_outputs_with_policy(&history, &policy_skip_small);
    assert_eq!(pruned, history);
}

#[test]
fn test_prune_policy_for_context_window() {
    let p_128k = PrunePolicy::for_context_window(128_000);
    assert_eq!(p_128k.min_total_savings, 6_400);
    assert_eq!(p_128k.protect_recent_tokens, 19_200);

    let p_200k = PrunePolicy::for_context_window(200_000);
    assert_eq!(p_200k.min_total_savings, 10_000);
    assert_eq!(p_200k.protect_recent_tokens, 30_000);

    let p_1m = PrunePolicy::for_context_window(1_000_000);
    assert_eq!(p_1m.min_total_savings, 20_000);
    assert_eq!(p_1m.protect_recent_tokens, 30_000);

    let p_tiny = PrunePolicy::for_context_window(32_000);
    assert_eq!(p_tiny.min_total_savings, 3_000);
    assert_eq!(p_tiny.protect_recent_tokens, 10_000);
}

#[test]
fn test_adaptive_pruning_floor_triggers_on_smaller_window() {
    let history = prior_turn_history(
        "Build",
        make_tool_turn(
            "c1",
            "bash",
            "cargo build",
            &"verbose compiler warning line output\n".repeat(1_600),
        ),
        "Done build",
        "Next command",
    );

    let static_policy = PrunePolicy {
        protect_recent_tokens: 0,
        ..PrunePolicy::cache_preserving()
    };
    let unpruned = prune_historical_tool_outputs_with_policy(&history, &static_policy);
    assert_eq!(unpruned, history);

    let adaptive_policy = PrunePolicy {
        protect_recent_tokens: 0,
        ..PrunePolicy::for_context_window(128_000)
    };
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
    let history = prior_turn_history(
        "Load skill",
        make_tool_turn(
            "c1",
            "skill",
            "coding-standard",
            &"Skill instructions line\n".repeat(50),
        ),
        "Skill loaded",
        "Next step",
    );

    let pruned = prune_historical_tool_outputs(&history);
    assert_eq!(pruned, history);
}

#[test]
fn test_superseded_file_reads_detected_and_elided() {
    let (edit_call_msg, edit_res_msg) = text_turn(
        "c2",
        "edit",
        serde_json::json!({ "path": "src/lib.rs", "content": "edited" }),
        "Successfully replaced",
    );
    let mut history = prior_turn_history(
        "Read file",
        make_read_turn("c1", "src/lib.rs", &"pub fn foo() {}\n".repeat(20)),
        "I read lib.rs",
        "Now edit it",
    );
    history.extend([
        edit_call_msg,
        edit_res_msg,
        ChatMessage::assistant("File edited"),
        ChatMessage::user("Next step"),
    ]);

    let pruned = prune_historical_tool_outputs(&history);
    assert_eq!(
        extract_tool_result_first_text(&pruned[2]),
        "[read 'src/lib.rs': superseded by a later call]"
    );
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
fn test_symbol_read_is_partial_and_never_supersedes_a_full_read() {
    let messages = vec![
        call_message("c1", "read", serde_json::json!({ "path": "src/lib.rs" })),
        call_message(
            "c2",
            "read",
            serde_json::json!({ "path": "src/lib.rs", "symbol": "Config.load" }),
        ),
    ];
    assert!(collect_superseded_tool_call_ids(&messages).is_empty());
}

fn ambiguous_symbol_listing() -> String {
    format!(
        "'load' matches 2 symbols; qualify it (e.g. Type.method) or read by offset:\n{}",
        "1-4 function load\n".repeat(80)
    )
}

#[test_case(&ambiguous_symbol_listing(); "ambiguous symbol")]
#[test_case("No symbol 'x' in src/lib.rs; lsp symbols with path lists them"; "missing symbol")]
#[test_case("symbol needs a language server, and none is enabled here"; "no language server")]
#[test_case("symbol cannot be combined with offset or limit; pass one or the other"; "symbol with offset")]
fn test_symbol_read_failures_are_not_pruned(output: &str) {
    let text = prune_prior_turn_text("Read", make_read_turn("c1", "src/lib.rs", output), "ok", "Next");
    assert_eq!(text, output);
}

#[test_case("edit", "Successfully applied 1 replacement(s) to src/lib.rs", true; "applied edit")]
#[test_case("edit", "Edit #1: could not find the text to replace", false; "failed edit")]
#[test_case("write", "Successfully wrote 4 bytes (1 lines) to src/lib.rs", true; "successful write")]
#[test_case("write", "Failed to write file src/lib.rs: denied", false; "failed write")]
#[test_case("read", "fn main() {}", true; "full reread")]
#[test_case("read", "line 1\n[Showing lines 1-2000 of 2500. Use offset=2001 to continue.]", false; "clipped reread")]
#[test_case("read", "File not found: src/lib.rs", false; "failed reread")]
fn a_later_call_supersedes_an_earlier_read_only_when_its_result_took_effect(
    later: &str,
    output: &str,
    superseded: bool,
) {
    let (earlier_call, earlier_result) = make_read_turn("c1", "src/lib.rs", "old content");
    let (later_call, later_result) = text_turn("c2", later, serde_json::json!({ "path": "src/lib.rs" }), output);

    let ids = collect_superseded_tool_call_ids(&[earlier_call, earlier_result, later_call, later_result]);

    assert_eq!(ids.contains("c1"), superseded);
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
    let text = prune_prior_turn_text(
        "Read file",
        make_read_turn("c1", "src/errors.rs", &source),
        "Read done",
        "Next",
    );

    assert!(text.contains(": pruned "));
}

fn verbose_read_history(turns: usize, reread_first_at: Option<usize>) -> Vec<ChatMessage> {
    let body = (0..20)
        .map(|i| format!("line {i} {}", "x".repeat(40)))
        .collect::<Vec<_>>()
        .join("\n");
    let mut history = vec![ChatMessage::user("start")];
    for n in 0..turns {
        let path = if reread_first_at == Some(n) {
            "src/f0.rs".to_string()
        } else {
            format!("src/f{n}.rs")
        };
        let (call, result) = make_read_turn(&format!("c{n}"), &path, &body);
        history.extend([
            call,
            result,
            ChatMessage::assistant(format!("done {n}")),
            ChatMessage::user(format!("next {n}")),
        ]);
    }
    history
}

fn cache_test_policy() -> PrunePolicy {
    PrunePolicy {
        protect_recent_tokens: 3_000,
        min_total_savings: 1_000,
        ..PrunePolicy::cache_preserving()
    }
    .with_model("claude-sonnet-4")
}

#[test]
fn test_pruned_prefix_changes_only_at_step_boundaries() {
    let policy = cache_test_policy();
    let turns = 40;
    let mut previous: Vec<ChatMessage> = Vec::new();
    let mut rewrites = 0;
    for n in 1..=turns {
        let pruned = prune_historical_tool_outputs_with_policy(&verbose_read_history(n, None), &policy);
        if pruned.len() < previous.len() || pruned[..previous.len()] != previous[..] {
            rewrites += 1;
        }
        previous = pruned;
    }
    assert!(
        rewrites <= turns / 4,
        "pruned prefix was rewritten on {rewrites} of {turns} turns"
    );
}

fn count_prefix_rewrites(
    turns: usize,
    reread_first_at: Option<usize>,
    mut send: impl FnMut(&[ChatMessage]) -> Vec<ChatMessage>,
) -> usize {
    let mut previous: Vec<ChatMessage> = Vec::new();
    let mut rewrites = 0;
    for n in 1..=turns {
        let sent = send(&verbose_read_history(n, reread_first_at));
        if sent[..previous.len()] != previous[..] {
            rewrites += 1;
        }
        previous = sent;
    }
    rewrites
}

fn send_with_memory(memory: &mut PruneMemory, history: &[ChatMessage], cache: PromptCache) -> Vec<ChatMessage> {
    decide_with_memory(memory, history, cache).0
}

fn decide_with_memory(
    memory: &mut PruneMemory,
    history: &[ChatMessage],
    cache: PromptCache,
) -> (Vec<ChatMessage>, PruneDecision) {
    let policy = cache_test_policy();
    let sizes = rho_harness_core::tokens::estimate_each_message_tokens(history, "claude-sonnet-4");
    let (sent, decision) = memory.prune(history, &sizes, &policy, cache);
    (sent.unwrap_or_else(|| history.to_vec()), decision)
}

#[test]
fn prune_decisions_report_cache_state_and_tokens() {
    let mut memory = PruneMemory::default();
    let (_, cold) = decide_with_memory(&mut memory, &verbose_read_history(30, None), PromptCache::Cold);
    assert_eq!(cold.action, PruneAction::Cold);

    let (_, same) = decide_with_memory(&mut memory, &verbose_read_history(30, None), PromptCache::Warm);
    assert_eq!(same.action, PruneAction::Unchanged);

    let (_, grown) = decide_with_memory(&mut memory, &verbose_read_history(45, None), PromptCache::Warm);
    assert!(matches!(grown.action, PruneAction::Kept | PruneAction::Rewritten));
    assert!(grown.rebuilt_tokens > 0);
    let rewrote = grown.saved_tokens as f64 >= grown.rebuilt_tokens as f64 * WARM_REBUILD_RATIO;
    assert_eq!(grown.action == PruneAction::Rewritten, rewrote);
}

#[test]
fn warm_cache_keeps_its_prefix_when_a_reread_only_relabels_an_old_stub() {
    let policy = cache_test_policy();
    let (turns, reread) = (50, Some(30));
    let stateless = count_prefix_rewrites(turns, reread, |h| prune_historical_tool_outputs_with_policy(h, &policy));
    let mut memory = PruneMemory::default();
    let warm = count_prefix_rewrites(turns, reread, |h| send_with_memory(&mut memory, h, PromptCache::Warm));

    assert!(warm < stateless, "warm rewrites {warm}, stateless rewrites {stateless}");
    let raw = verbose_read_history(turns, reread);
    let sent = send_with_memory(&mut memory, &raw, PromptCache::Warm);
    let tokens = |m: &[ChatMessage]| {
        rho_harness_core::tokens::estimate_each_message_tokens(m, "claude-sonnet-4")
            .iter()
            .sum::<usize>()
    };
    assert!(tokens(&sent) * 2 < tokens(&raw), "warm history was not pruned");
}

#[test]
fn cold_cache_prunes_from_scratch() {
    let policy = cache_test_policy();
    let mut memory = PruneMemory::default();
    for n in [20, 21, 35] {
        let history = verbose_read_history(n, None);
        let cold = send_with_memory(&mut memory, &history, PromptCache::Cold);
        assert_eq!(
            cold,
            prune_historical_tool_outputs_with_policy(&history, &policy),
            "{n} turns"
        );
    }
}

#[test]
fn warm_cache_replays_stubs_it_sent_before() {
    let mut memory = PruneMemory::default();
    let first = send_with_memory(&mut memory, &verbose_read_history(30, None), PromptCache::Cold);
    let next = send_with_memory(&mut memory, &verbose_read_history(31, None), PromptCache::Warm);

    assert!(extract_tool_result_first_text(&first[2]).contains(": pruned "));
    assert_eq!(next[..first.len()], first[..]);
}

#[test]
fn test_reread_in_protected_tail_does_not_rewrite_pruned_prefix() {
    let policy = cache_test_policy();
    let before = prune_historical_tool_outputs_with_policy(&verbose_read_history(30, None), &policy);
    let after = prune_historical_tool_outputs_with_policy(&verbose_read_history(30, Some(29)), &policy);

    assert!(extract_tool_result_first_text(&before[2]).contains(": pruned "));
    assert_eq!(before[2], after[2]);
}

#[test]
fn test_pruned_read_stub_keeps_artifact_path_from_spill_notice() {
    let body = (1..=40).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let notice = "\n\n[Output truncated. Full content (900 lines, 40.0KB) saved to .rho/artifacts/abc.log. Use 'read' or 'bash' with grep on this file if needed.]";
    let spilled = format!("{body}{notice}");
    let plain = body.clone();

    let prune = |tool: &str, text: &str| {
        prune_prior_turn_text("search", make_tool_turn("c1", tool, "needle", text), "done", "next")
    };
    let read = prune("read", &spilled);
    assert!(read.contains("; full: .rho/artifacts/abc.log]"), "{read}");
    assert!(!prune("read", &plain).contains("artifacts"));
}

#[test]
fn test_find_recent_tokens_cutoff_uses_precomputed_sizes() {
    let sizes = [10, 20, 30, 20];
    assert_eq!(find_recent_tokens_cutoff(&sizes, 40), 2);
    assert_eq!(find_recent_tokens_cutoff(&sizes, 0), sizes.len());
    assert_eq!(find_recent_tokens_cutoff(&[], 40), 0);
}

#[test]
fn precomputed_sizes_match_policy_pruning() {
    let policy = cache_test_policy();
    let messages = verbose_read_history(30, None);
    let sizes = rho_harness_core::tokens::estimate_each_message_tokens(&messages, "claude-sonnet-4");
    let expected = prune_historical_tool_outputs_with_policy(&messages, &policy);
    assert_ne!(expected, messages);
    assert_eq!(
        prune_with_token_cache(&messages, &sizes, &policy, &mut MessageTokenCache::new()),
        Some(expected)
    );
}

#[test]
fn repeated_prune_of_unchanged_prefix_does_not_retokenize() {
    let policy = cache_test_policy();
    let history = verbose_read_history(30, None);
    let sizes = rho_harness_core::tokens::estimate_each_message_tokens(&history, "claude-sonnet-4");
    let mut memo = MessageTokenCache::new();
    let first = prune_with_token_cache(&history, &sizes, &policy, &mut memo);
    let misses = memo.misses();
    assert!(misses > 0);

    let second = prune_with_token_cache(&history, &sizes, &policy, &mut memo);
    assert_eq!(second, first);
    assert_eq!(memo.misses(), misses);
}

#[test]
fn repeated_prune_of_long_history_does_not_retokenize() {
    let policy = cache_test_policy();
    let history = verbose_read_history(600, None);
    assert!(history.len() > rho_harness_core::tokens::DEFAULT_CACHE_CAPACITY);
    let sizes = rho_harness_core::tokens::estimate_each_message_tokens(&history, "claude-sonnet-4");
    let mut memo = MessageTokenCache::new();
    let first = prune_with_token_cache(&history, &sizes, &policy, &mut memo);
    let misses = memo.misses();
    assert!(misses > 0);

    let second = prune_with_token_cache(&history, &sizes, &policy, &mut memo);
    assert_eq!(second, first);
    assert_eq!(memo.misses(), misses);
}

#[test]
fn prune_with_token_cache_returns_none_when_nothing_is_pruned() {
    let messages = vec![
        ChatMessage::User {
            content: vec![UserContent::Text(TextContent::new("hello"))],
        },
        ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::Text(TextContent::new("hi there"))],
        },
    ];
    let policy = unthrottled_policy();
    let sizes = rho_harness_core::tokens::estimate_each_message_tokens(&messages, policy.tokenizer_model());
    assert!(prune_with_token_cache(&messages, &sizes, &policy, &mut MessageTokenCache::new()).is_none());
    assert_eq!(prune_historical_tool_outputs_with_policy(&messages, &policy), messages);
}
