use super::*;
use test_case::test_case;

#[test]
fn summarize_tool_output_short_ascii() {
    assert_eq!(summarize_tool_output("hello world"), "hello world");
}

#[test]
fn summarize_tool_output_truncates_ascii_over_60() {
    let input = "a".repeat(65);
    assert_eq!(summarize_tool_output(&input), format!("{}...", "a".repeat(60)));
}

#[test]
fn summarize_tool_output_multibyte_boundary_does_not_panic() {
    // 59 ASCII bytes followed by 4-byte unicode characters (e.g. emoji or multi-byte UTF-8).
    // Byte index 60 falls inside the first emoji.
    let input = format!("{}🦀🦀🦀", "a".repeat(59));
    let summary = summarize_tool_output(&input);
    assert_eq!(summary, format!("{}🦀...", "a".repeat(59)));
}

#[test]
fn summarize_tool_output_multibyte_cjk_over_60_chars() {
    let input = "日本語".repeat(30); // 90 CJK characters (270 bytes)
    let summary = summarize_tool_output(&input);
    let expected: String = input.chars().take(60).collect();
    assert_eq!(summary, format!("{expected}..."));
}

#[test]
fn summarize_tool_output_empty_and_multiline() {
    assert_eq!(summarize_tool_output(""), "0 lines");
    assert_eq!(summarize_tool_output("\n\n"), "2 lines");
    assert_eq!(summarize_tool_output("first line\nsecond line"), "first line");
}

#[test]
fn format_tool_args_summary_bash_multibyte_truncation() {
    let cmd = format!("echo {}", "🦀".repeat(70));
    let args = serde_json::json!({ "command": cmd });
    let summary = format_tool_args_summary("bash", &args);
    assert!(summary.starts_with("echo "));
    assert!(summary.ends_with("..."));
    assert!(!summary.contains('`'));
}

#[test]
fn format_tool_args_summary_fd() {
    let with_pattern = serde_json::json!({ "pattern": "widget", "path": "src" });
    assert_eq!(format_tool_args_summary("fd", &with_pattern), "widget src");

    let with_spaced_pattern = serde_json::json!({ "pattern": "my widget", "path": "src" });
    assert_eq!(format_tool_args_summary("fd", &with_spaced_pattern), "'my widget' src");

    let without_pattern = serde_json::json!({ "path": "src" });
    assert_eq!(format_tool_args_summary("fd", &without_pattern), ". src");

    let default_root = serde_json::json!({});
    assert_eq!(format_tool_args_summary("fd", &default_root), ".");
}

#[test]
fn format_tool_args_summary_rg() {
    let simple = serde_json::json!({ "pattern": "foo", "path": "src" });
    assert_eq!(format_tool_args_summary("rg", &simple), "foo src");

    let root_path = serde_json::json!({ "pattern": "foo", "path": "." });
    assert_eq!(format_tool_args_summary("rg", &root_path), "foo");

    let spaced = serde_json::json!({ "pattern": "hello world", "path": "src" });
    assert_eq!(format_tool_args_summary("rg", &spaced), "'hello world' src");

    let regex_chars = serde_json::json!({ "pattern": "fn.*run_turn", "path": "." });
    assert_eq!(format_tool_args_summary("rg", &regex_chars), "'fn.*run_turn'");
}

#[test_case("", "''"; "empty")]
#[test_case("foo", "foo"; "plain word")]
#[test_case("foo-bar_1.2", "foo-bar_1.2"; "safe punctuation")]
#[test_case("foo bar", "'foo bar'"; "space")]
#[test_case("don't", "'don'\\''t'"; "single quote")]
fn test_quote_cli_arg(input: &str, expected: &str) {
    assert_eq!(quote_cli_arg(input), expected);
}

#[test]
fn format_tool_args_summary_mcp_call() {
    let call = serde_json::json!({
        "action": "call",
        "server": "playwright",
        "tool": "browser_navigate",
        "args": {
            "url": "http://localhost:3000/hub/"
        }
    });
    assert_eq!(
        format_tool_args_summary("mcp", &call),
        "playwright:browser_navigate url=\"http://localhost:3000/hub/\""
    );

    let no_args = serde_json::json!({
        "server": "playwright",
        "tool": "browser_snapshot"
    });
    assert_eq!(format_tool_args_summary("mcp", &no_args), "playwright:browser_snapshot");

    let no_server = serde_json::json!({
        "tool": "browser_navigate",
        "args": {
            "url": "http://localhost:3000/hub/"
        }
    });
    assert_eq!(
        format_tool_args_summary("mcp", &no_server),
        "browser_navigate url=\"http://localhost:3000/hub/\""
    );

    let multiple_args = serde_json::json!({
        "action": "call",
        "server": "playwright",
        "tool": "browser_click",
        "args": {
            "element": "Add Node button",
            "target": "f1e7"
        }
    });
    assert_eq!(
        format_tool_args_summary("mcp", &multiple_args),
        "playwright:browser_click element=\"Add Node button\" target=\"f1e7\""
    );

    let top_level_args = serde_json::json!({
        "server": "playwright",
        "tool": "browser_navigate",
        "url": "http://localhost:3000/hub/"
    });
    assert_eq!(
        format_tool_args_summary("mcp", &top_level_args),
        "playwright:browser_navigate url=\"http://localhost:3000/hub/\""
    );
}

#[test]
fn format_tool_args_summary_mcp_actions() {
    let status_with_server = serde_json::json!({
        "action": "status",
        "server": "playwright"
    });
    assert_eq!(
        format_tool_args_summary("mcp", &status_with_server),
        "status playwright"
    );

    let status_all = serde_json::json!({
        "action": "status"
    });
    assert_eq!(format_tool_args_summary("mcp", &status_all), "status");

    let search = serde_json::json!({
        "action": "search",
        "server": "playwright",
        "search": "navigate"
    });
    assert_eq!(
        format_tool_args_summary("mcp", &search),
        "playwright:search \"navigate\""
    );
    let search_no_server = serde_json::json!({
        "search": "navigate"
    });
    assert_eq!(
        format_tool_args_summary("mcp", &search_no_server),
        "search \"navigate\""
    );

    let describe = serde_json::json!({
        "action": "describe",
        "server": "playwright",
        "describe": "browser_navigate"
    });
    assert_eq!(
        format_tool_args_summary("mcp", &describe),
        "playwright:describe browser_navigate"
    );

    let describe_no_server = serde_json::json!({
        "describe": "browser_navigate"
    });
    assert_eq!(
        format_tool_args_summary("mcp", &describe_no_server),
        "describe browser_navigate"
    );

    let custom_action = serde_json::json!({
        "action": "restart",
        "server": "playwright"
    });
    assert_eq!(format_tool_args_summary("mcp", &custom_action), "playwright:restart");

    let custom_action_no_server = serde_json::json!({
        "action": "restart"
    });
    assert_eq!(format_tool_args_summary("mcp", &custom_action_no_server), "restart");

    let server_only = serde_json::json!({
        "server": "playwright"
    });
    assert_eq!(format_tool_args_summary("mcp", &server_only), "playwright");

    let empty = serde_json::json!({});
    assert_eq!(format_tool_args_summary("mcp", &empty), "");

    let empty_tool = serde_json::json!({
        "action": "call",
        "tool": "",
        "server": "playwright"
    });
    assert_eq!(format_tool_args_summary("mcp", &empty_tool), "playwright:call");
}

#[test]
fn format_tool_args_summary_mcp_script() {
    let single = serde_json::json!({
        "calls": [
            {
                "server": "playwright",
                "tool": "browser_navigate",
                "args": { "url": "http://localhost:3000/hub/" }
            }
        ]
    });
    assert_eq!(
        format_tool_args_summary("mcpScript", &single),
        "playwright:browser_navigate url=\"http://localhost:3000/hub/\""
    );

    let multiple = serde_json::json!({
        "calls": [
            { "server": "playwright", "tool": "browser_navigate" },
            { "server": "playwright", "tool": "browser_snapshot" }
        ]
    });
    assert_eq!(
        format_tool_args_summary("mcpScript", &multiple),
        "[2 calls] playwright:browser_navigate, playwright:browser_snapshot"
    );
}

#[test]
fn format_tool_args_summary_direct_mcp_or_generic() {
    let direct = serde_json::json!({
        "url": "http://localhost:3000/hub/"
    });
    assert_eq!(
        format_tool_args_summary("playwright_browser_navigate", &direct),
        "url=\"http://localhost:3000/hub/\""
    );

    let empty = serde_json::json!({});
    assert_eq!(format_tool_args_summary("custom_tool", &empty), "");
}

#[test]
fn format_tool_args_summary_sanitization_and_truncation() {
    let args = serde_json::json!({
        "query": "line1\r\nline2 \"quoted\""
    });
    let formatted = format_tool_args_summary("custom", &args);
    assert_eq!(formatted, r#"query="line1 line2 \"quoted\"""#);

    let long_arg = serde_json::json!({
        "long": "this is a very long string that definitely exceeds the forty character limit for kv formatting"
    });
    let long_formatted = format_tool_args_summary("custom", &long_arg);
    assert!(long_formatted.contains("..."));
}

#[test]
fn format_tool_args_summary_todo_shows_progress_and_the_active_item() {
    let todos = |statuses: &[&str]| {
        let items: Vec<_> = statuses
            .iter()
            .enumerate()
            .map(|(idx, status)| serde_json::json!({ "content": format!("Step {idx}"), "status": status }))
            .collect();
        serde_json::json!({ "todos": items })
    };

    assert_eq!(
        format_tool_args_summary("todo", &todos(&["completed", "in_progress", "pending"])),
        "1/3 done \u{b7} Step 1"
    );
    assert_eq!(
        format_tool_args_summary("todo", &todos(&["completed", "completed"])),
        "2/2 done"
    );
    assert_eq!(format_tool_args_summary("todo", &serde_json::json!({})), "");
}

#[test]
fn format_tool_args_summary_names_undo_lsp_memory_and_symbol_reads() {
    let summary = |name, args: serde_json::Value| format_tool_args_summary(name, &args);
    assert_eq!(summary("undo", serde_json::json!({ "path": "src/a.rs" })), "src/a.rs");
    assert_eq!(
        summary(
            "lsp",
            serde_json::json!({ "action": "references", "path": "src/a.rs", "line": 12, "symbol": "parse" })
        ),
        "references src/a.rs:12 parse"
    );
    assert_eq!(
        summary(
            "lsp",
            serde_json::json!({ "action": "diagnostics", "path": "src/a.rs" })
        ),
        "diagnostics src/a.rs"
    );
    assert_eq!(
        summary("lsp", serde_json::json!({ "action": "symbols", "query": "Config" })),
        "symbols Config"
    );
    assert_eq!(summary("lsp", serde_json::json!({ "action": "status" })), "status");
    assert_eq!(
        summary("memory", serde_json::json!({ "action": "add", "text": "x" })),
        "add project"
    );
    assert_eq!(
        summary("memory", serde_json::json!({ "action": "view", "scope": "global" })),
        "view global"
    );
    assert_eq!(
        summary(
            "read",
            serde_json::json!({ "path": "src/a.rs", "symbol": "Config::load", "offset": 5 })
        ),
        "src/a.rs symbol=Config::load"
    );
}
