use super::card::{CardStatus, CardView, fetch_content_kind, format_bash_args_header};
use super::diff::format_gutter_prefix;
use super::formatters::{
    EditDiffRequest, WritePreview, format_edit_diff, format_read_expanded, format_relative_time, format_session_status,
    format_write_preview,
};
use super::notices::{NoticeKind, format_notice};
use super::{format_tool_header, render_tool_transcript};
use crate::ui::TerminalRenderer;
use crate::ui::block::{strip_ansi, visible_width};
use crate::ui::interactive::{
    Activity, InteractiveUi, OutputEvent, TranscriptItem, TranscriptRenderInput, UiEvent, render_transcript_item,
};
use crate::ui::render::card::render_tool_block;
use crate::ui::theme::Theme;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rho_harness_core::presentation::summary::{
    ReadClassification, classify_read_path, clean_command_paths, format_tool_args_summary, read_summary_parts,
    to_relative_path,
};
use rho_harness_core::presentation::{BlockDisplay, FileChange, SessionStatus, ToolLine, WelcomeDisplay};
use test_case::test_case;

fn assert_contains_all(s: &str, items: &[&str]) {
    for item in items {
        assert!(s.contains(item));
    }
}

// =========================================================================
// Formatters Tests
// =========================================================================

fn edit_diff(args: &serde_json::Value, theme: &Theme) -> Option<String> {
    let request = EditDiffRequest {
        args,
        file_change: None,
        expanded: false,
        width: 60,
    };
    format_edit_diff(request, theme)
}

#[test]
fn test_format_edit_diff_renders_removals_and_additions() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "src/main.rs",
        "edits": [
            {
                "oldText": "let x = 1;",
                "newText": "let x = 2;\nlet y = 3;"
            }
        ]
    });
    let diff = edit_diff(&args, &theme).unwrap();
    assert!(!diff.contains("```diff") && !diff.contains("```"));
    assert_contains_all(&diff, &["- let x = 1;", "+ let x = 2;", "+ let y = 3;"]);
    assert!(diff.ends_with('\n'));
}

#[test]
fn test_format_edit_diff_intra_line_word_highlighting() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "src/main.rs",
        "edits": [
            {
                "oldText": "    let old_val = 10;",
                "newText": "    let new_val = 10;"
            }
        ]
    });
    let diff = edit_diff(&args, &theme).unwrap();
    assert!(!diff.contains("```diff") && !diff.contains("```"));
    assert_contains_all(
        &diff,
        &[
            "-     let ",
            "+     let ",
            "\x1b[7mold_val\x1b[27m",
            "\x1b[7mnew_val\x1b[27m",
            " = 10;",
        ],
    );
}

#[test]
fn test_format_write_preview_syntax_highlighting() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "test.py",
        "content": "def main():\n    print('hello')"
    });
    let preview = write_preview(&args, &theme, false, 60).unwrap();
    assert!(!preview.contains("```diff") && !preview.contains("```") && !preview.contains("+ def main():"));
    assert_contains_all(&preview, &["def", "main", "print", "hello", "  1 │ ", "  2 │ "]);
}

#[test]
fn file_card_headers_state_each_count_once() {
    let theme = Theme::default();
    let header = |name: &str, arguments: serde_json::Value| {
        let tool = ToolLine::new(name, arguments);
        let card = strip_ansi(&render_tool_block(&tool, 100, false, false, &theme));
        card.lines().nth(1).unwrap().to_string()
    };
    let edit = header(
        "edit",
        serde_json::json!({"path": "a.rs", "edits": [{"oldText": "a", "newText": "b"}, {"oldText": "c", "newText": "d"}]}),
    );
    assert_eq!(edit.matches("2 edits").count(), 1, "{edit}");
    assert!(!edit.contains("(2 edits)"), "{edit}");
    let write = header("write", serde_json::json!({"path": "a.txt", "text": "one\ntwo"}));
    assert!(!write.contains("bytes)"), "{write}");
    assert!(write.contains("2 lines"), "{write}");
}

#[test_case("head", "a"; "no trailing newlines")]
#[test_case("head\n", "a\n"; "one trailing newline")]
#[test_case("head\n\n\n", "a\n\n"; "extra trailing newlines")]
fn push_section_always_leaves_exactly_one_blank_row(body: &str, section: &str) {
    use crate::ui::render::card::push_section;
    let mut content = body.to_string();
    push_section(&mut content, section);
    push_section(&mut content, "b\n");
    assert_eq!(content, "head\n\na\n\nb");
}

#[test]
fn every_card_has_exactly_one_blank_row_before_the_duration() {
    let long = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let edit = serde_json::json!({"path": "a.rs", "edits": [{"oldText": "let x = 1;", "newText": "let x = 2;"}]});
    let cases = [
        ("edit", edit.clone(), "", false),
        ("edit", edit, "Error: no match", true),
        ("write", serde_json::json!({"path": "a.txt", "text": "one"}), "", false),
        ("write", serde_json::json!({"path": "a.txt", "text": long}), "", false),
        ("bash", serde_json::json!({"command": "ls"}), "a\nb\n", false),
        ("bash", serde_json::json!({"command": "ls"}), "a\nb\n\n\n", false),
        ("bash", serde_json::json!({"command": "ls"}), "a\nb", false),
        ("bash", serde_json::json!({"command": "false"}), "boom\n", true),
        ("read", serde_json::json!({"path": "a.rs"}), "1\tfn main() {}\n", false),
        ("grep", serde_json::json!({"pattern": "x"}), "a.rs:1:x\n", false),
    ];
    for structured_tool_cards in [false, true] {
        let theme = Theme {
            structured_tool_cards,
            ..Theme::default()
        };
        for (name, arguments, output, is_error) in &cases {
            for expanded in [false, true] {
                let mut tool = ToolLine::new(*name, arguments.clone())
                    .with_output(*output)
                    .with_duration_ms(2000);
                tool.is_error = *is_error;
                let card = strip_ansi(&render_tool_block(&tool, 60, expanded, false, &theme));
                let rows: Vec<&str> = card.lines().collect();
                let Some(took) = rows.iter().position(|r| r.contains("Took ")) else {
                    continue;
                };
                let blanks = rows[..took]
                    .iter()
                    .rev()
                    .take_while(|r| {
                        r.trim_matches(|c: char| c == '│' || c.is_whitespace() || c == '\u{a0}')
                            .is_empty()
                    })
                    .count();
                assert_eq!(
                    blanks, 1,
                    "{name} error={is_error} expanded={expanded} structured={structured_tool_cards}:\n{card}"
                );
            }
        }
    }
}

#[test]
fn test_format_write_preview_expansion_and_collapse() {
    let theme = Theme::default();
    let long_content = (1..=12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let long_args = serde_json::json!({
        "path": "test.txt",
        "content": long_content
    });
    let collapsed = strip_ansi(&write_preview(&long_args, &theme, false, 60).unwrap());
    let first = collapsed.lines().next().unwrap();
    assert_eq!(first, "... (2 earlier lines, 12 total, Ctrl+O to expand)");
    assert!(collapsed.contains("  3 │ line 3") && collapsed.contains(" 12 │ line 12"));
    assert!(!collapsed.contains("line 2\n"), "{collapsed}");

    let expanded = strip_ansi(&write_preview(&long_args, &theme, true, 60).unwrap());
    assert!(!expanded.contains("earlier lines"), "{expanded}");
    assert!(expanded.contains("  1 │ line 1\n") && expanded.contains(" 12 │ line 12"));
}

#[test]
fn collapsed_write_preview_grows_to_ten_rows_and_never_shrinks() {
    let theme = Theme::default();
    let long_line = "let value = compute_something_long(argument_one, argument_two, argument_three);";
    let heights: Vec<usize> = (1..=40usize)
        .map(|n| {
            let text = (0..n).map(|_| long_line).collect::<Vec<_>>().join("\n");
            let args = serde_json::json!({"path": "a.rs", "text": text});
            let preview = write_preview(&args, &theme, false, 40).unwrap();
            let plain = strip_ansi(&preview);
            assert!(plain.lines().all(|l| visible_width(l) <= 40), "{plain}");
            plain.lines().count()
        })
        .collect();
    assert!(heights.windows(2).all(|w| w[0] <= w[1]), "{heights:?}");
    assert_eq!(heights[..10], [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    assert!(heights[10..].iter().all(|&h| h == 11), "{heights:?}");
}

#[test]
fn test_format_edit_diff_with_explicit_line_number() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "src/main.rs",
        "edits": [
            {
                "oldText": "let a = 1;\nlet b = 2;",
                "newText": "let a = 10;\nlet b = 20;",
                "line": 42
            }
        ]
    });
    let diff = edit_diff(&args, &theme).unwrap();
    assert!(diff.contains(" 42 │ "));
    assert!(diff.contains(" 43 │ "));
    assert!(diff.contains("- let a = 1;"));
    assert!(diff.contains("+ let a = 10;"));
}

#[test]
fn test_format_edit_diff_locates_line_from_file_on_disk() {
    let theme = Theme::default();
    let temp_dir = rho_test_support::TestDir::new();
    let file_path = temp_dir.join("example.rs");
    std::fs::write(&file_path, "line 1\nline 2\nline 3\ntarget line\nline 5\n").unwrap();

    let path_str = file_path.to_str().unwrap();
    let args = serde_json::json!({
        "path": path_str,
        "edits": [
            {
                "oldText": "target line",
                "newText": "replaced line"
            }
        ]
    });

    let diff_before = edit_diff(&args, &theme).unwrap();
    assert_contains_all(&diff_before, &["  4 │ ", "target", "replaced"]);

    std::fs::write(&file_path, "line 1\nline 2\nline 3\nreplaced line\nline 5\n").unwrap();
    let diff_after = edit_diff(&args, &theme).unwrap();
    assert_contains_all(&diff_after, &["  4 │ ", "target", "replaced"]);
}

#[test]
fn test_format_edit_diff_multi_edit_resolves_lines_from_single_read() {
    let theme = Theme::default();
    let temp_dir = rho_test_support::TestDir::new();
    let file_path = temp_dir.join("example_multi.rs");
    std::fs::write(&file_path, "fn one() {}\n\nfn two() {}\n").unwrap();

    let path_str = file_path.to_str().unwrap();
    let args = serde_json::json!({
        "path": path_str,
        "edits": [
            { "oldText": "fn one() {}", "newText": "fn first() {}" },
            { "oldText": "fn two() {}", "newText": "fn second() {}" }
        ]
    });

    let diff = edit_diff(&args, &theme).unwrap();
    assert_contains_all(&diff, &["  1 │ ", "  3 │ ", "first", "second"]);
}

#[test]
fn test_format_gutter_prefix_formats_aligned_and_dimmed() {
    let dim = anstyle::Style::new().dimmed();
    let prefix = format_gutter_prefix(1, 3, dim);
    assert_eq!(prefix, format!("{dim}  1 │ {dim:#}"));

    let prefix_wide = format_gutter_prefix(1234, 4, dim);
    assert_eq!(prefix_wide, format!("{dim}1234 │ {dim:#}"));
}

#[test_case(
    "     1\tfn main() {\n     2\t    println!(\"hello\");\n     3\t}\n",
    serde_json::json!({ "path": "src/main.rs" }),
    &["  1 │ ", "  2 │ ", "  3 │ ", "main", "println"];
    "numbered rows"
)]
#[test_case(
    "   100\tlet a = 1;\n   101\tlet b = 2;\n",
    serde_json::json!({ "path": "src/main.rs", "offset": 100 }),
    &["100 │ ", "101 │ "];
    "offset rows"
)]
#[test_case(
    "fn main() {\n    println!(\"hello\");\n}\n",
    serde_json::json!({ "path": "src/main.rs" }),
    &["  1 │ ", "  2 │ ", "  3 │ "];
    "unnumbered rows"
)]
fn format_read_expanded_numbers_every_row(raw: &str, args: serde_json::Value, expected: &[&str]) {
    let theme = Theme::default();
    let formatted = format_read_expanded(raw, &args, &theme).unwrap();
    assert_contains_all(&formatted, expected);
}

#[test]
fn test_format_read_expanded_with_continuation_notice() {
    let theme = Theme::default();
    let args = serde_json::json!({ "path": "src/main.rs" });
    let raw =
        "     1\tfn main() {\n     2\t    println!(\"hello\");\n\n[10 more lines in file. Use offset=3 to continue.]\n";
    let formatted = format_read_expanded(raw, &args, &theme).unwrap();
    assert_contains_all(
        &formatted,
        &["  1 │ ", "  2 │ ", "[10 more lines in file. Use offset=3 to continue.]"],
    );
    assert!(!formatted.contains("  3 │ [10 more lines"));
}

#[test]
fn test_line_number_gutter_parity_across_read_edit_write() {
    let theme = Theme::default();

    let write_args = serde_json::json!({ "path": "example.rs", "content": "fn test() {}" });
    let write_out = write_preview(&write_args, &theme, false, 60).unwrap();

    let edit_args = serde_json::json!({
        "path": "example.rs",
        "edits": [{ "oldText": "fn old() {}", "newText": "fn test() {}", "line": 1 }]
    });
    let edit_out = edit_diff(&edit_args, &theme).unwrap();

    let read_args = serde_json::json!({ "path": "example.rs" });
    let read_raw = "     1\tfn test() {}\n";
    let read_out = format_read_expanded(read_raw, &read_args, &theme).unwrap();

    assert!(write_out.contains("  1 │ "));
    assert!(edit_out.contains("  1 │ "));
    assert!(read_out.contains("  1 │ "));
}

#[test]
fn relative_time_buckets_and_boundaries() {
    let now = Utc::now();
    let old = now - ChronoDuration::days(40);
    let cases: [(DateTime<Utc>, String); 15] = [
        (now, "just now".to_string()),
        (now - ChronoDuration::seconds(30), "just now".to_string()),
        (now - ChronoDuration::seconds(59), "just now".to_string()),
        (now - ChronoDuration::seconds(60), "1m ago".to_string()),
        (now - ChronoDuration::minutes(5), "5m ago".to_string()),
        (now - ChronoDuration::minutes(59), "59m ago".to_string()),
        (now - ChronoDuration::minutes(60), "1h ago".to_string()),
        (now - ChronoDuration::hours(3), "3h ago".to_string()),
        (now - ChronoDuration::hours(23), "23h ago".to_string()),
        (now - ChronoDuration::hours(24), "1d ago".to_string()),
        (now - ChronoDuration::days(2), "2d ago".to_string()),
        (now - ChronoDuration::days(4), "4d ago".to_string()),
        (now - ChronoDuration::days(10), "10d ago".to_string()),
        (now - ChronoDuration::days(29), "29d ago".to_string()),
        (
            now - ChronoDuration::days(30),
            (now - ChronoDuration::days(30)).format("%Y-%m-%d").to_string(),
        ),
    ];
    for (time, expected) in cases {
        assert_eq!(format_relative_time(time), expected);
    }
    assert_eq!(format_relative_time(old), old.format("%Y-%m-%d").to_string());
    assert!(format_relative_time(now - ChronoDuration::days(45)).contains('-'));
}

#[test_case("claude-sonnet", "test", "27.4% (1M)", None, "claude-sonnet | 27.4% (1M)"; "no quota")]
#[test_case(
    "claude-sonnet",
    "anthropic",
    "27.4% (1M)",
    Some("93% (3h22m)"),
    "claude-sonnet | 27.4% (1M) | 93% (3h22m)";
    "with quota"
)]
#[test_case(
    "qwen-日本語モデル",
    "test",
    "0% (376k)",
    Some("80% quota"),
    "qwen-日本語モデル | 0% (376k) | 80% quota";
    "unicode model"
)]
#[test_case("qwen", "ollama", "0% (376k)", None, "qwen | 0% (376k)"; "local model")]
fn session_status_joins_model_context_and_optional_quota(
    model: &str,
    provider: &str,
    context: &str,
    quota: Option<&str>,
    expected: &str,
) {
    let session = SessionStatus {
        model: model.to_string(),
        provider: provider.to_string(),
        context: context.to_string(),
        quota: quota.map(str::to_string),
    };
    assert_eq!(format_session_status(&session), expected);
}

// =========================================================================
// Interactive Renderer Tests
// =========================================================================

fn renderer_with_events() -> (TerminalRenderer, tokio::sync::mpsc::UnboundedReceiver<UiEvent>) {
    let (ui, events) = InteractiveUi::channel();
    (TerminalRenderer::with_ui(ui), events)
}

fn drain_events(events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>) -> Vec<UiEvent> {
    std::iter::from_fn(|| events.try_recv().ok()).collect()
}

fn notice_texts(events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>) -> Vec<String> {
    drain_events(events)
        .into_iter()
        .filter_map(|event| match event {
            UiEvent::Transcript(TranscriptItem::Notice(text)) => Some(text),
            _ => None,
        })
        .collect()
}

fn finished_output(tool: ToolLine) -> String {
    let (renderer, mut events) = renderer_with_events();
    renderer.finish_tool_line(tool);
    drain_interactive_output(&mut events, &renderer.theme).1
}

fn structured_theme() -> Theme {
    Theme {
        structured_tool_cards: true,
        ..Theme::default()
    }
}

fn drain_interactive_output(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    theme: &Theme,
) -> (Vec<Activity>, String) {
    let mut activity_events = Vec::new();
    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Activity(activity) => activity_events.push(activity),
            UiEvent::Transcript(item) => output.push_str(&render_transcript_item(TranscriptRenderInput {
                item: &item,
                theme,
                width: 80,
                tools_expanded: false,
                hide_thinking: false,
            })),
            UiEvent::Output(OutputEvent::Text(text) | OutputEvent::StreamText(text)) => output.push_str(&text),
            _ => {}
        }
    }
    (activity_events, output)
}

fn drain_text_output(events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>) -> String {
    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        if let UiEvent::Output(OutputEvent::Text(text) | OutputEvent::StreamText(text)) = event {
            output.push_str(&text);
        }
    }
    output
}

#[test]
fn interactive_renderer_marks_assistant_tokens_as_stream_output() {
    let (renderer, mut events) = renderer_with_events();

    renderer.print_token("answer");

    assert!(matches!(
        events.try_recv(),
        Ok(UiEvent::Output(OutputEvent::StreamText(text))) if text == " answer"
    ));
}

#[test]
fn interactive_renderer_emits_formatted_output_and_activity_events() {
    let (renderer, mut events) = renderer_with_events();

    let activity = renderer.start_spinner("thinking...");
    renderer.print_thinking_token("considering");
    activity.finish_and_clear();
    renderer.print_token("answer");
    renderer.flush();
    renderer.finish_tool_line(
        ToolLine::new("read", serde_json::json!({"path": "src/lib.rs"}))
            .with_output("contents")
            .with_output_summary("contents"),
    );

    let (activity_events, output) = drain_interactive_output(&mut events, &renderer.theme);
    assert_eq!(activity_events, [Activity::Thinking, Activity::Idle]);
    for token in ["considering", "answer", "read", "src/lib.rs"] {
        assert!(output.contains(token));
    }
}

#[test]
fn renderer_flush_resets_markdown_state_between_turns() {
    let (renderer, mut events) = renderer_with_events();

    renderer.print_token("First response.\n");
    renderer.flush();
    while events.try_recv().is_ok() {}

    renderer.print_token("# Second Turn Title\n");
    renderer.flush();

    let turn2_output = drain_text_output(&mut events);
    assert!(
        !turn2_output.starts_with('\n'),
        "expected no extra leading newline, got: {turn2_output:?}"
    );
    assert!(turn2_output.contains("Second Turn Title"));
}

fn assert_fast_tool_run(
    renderer: &TerminalRenderer,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    tool: &str,
    args: &serde_json::Value,
) {
    renderer.start_tool_run(tool, args);
    assert!(matches!(events.try_recv(), Ok(UiEvent::RunningTool(Some(name))) if name == tool));
    assert!(events.try_recv().is_err());
}

#[test]
fn fast_tools_emit_footer_status_without_live_widget_bounce() {
    let (renderer, mut events) = renderer_with_events();

    for tool in ["read", "rg"] {
        assert_fast_tool_run(
            &renderer,
            &mut events,
            tool,
            &serde_json::json!({"path": "src/main.rs"}),
        );
    }
}

#[test]
fn long_running_tools_open_live_card() {
    let (renderer, mut events) = renderer_with_events();
    let cases = [
        ("bash", serde_json::json!({"command": "cargo test"})),
        ("edit", serde_json::json!({"path": "src/main.rs", "edits": []})),
        ("subagent", serde_json::json!({"prompt": "find callers"})),
    ];
    for (tool, args) in cases {
        renderer.start_tool_run(tool, &args);
        match events.try_recv() {
            Ok(UiEvent::ToolStart(request)) => assert_eq!(request.name, tool),
            other => panic!("expected ToolStart for {tool}, got ok={}", other.is_ok()),
        }
        assert!(events.try_recv().is_err());
    }
}

#[test]
fn finished_bash_block_includes_elapsed_duration() {
    let output = finished_output(
        ToolLine::new("bash", serde_json::json!({"command": "cargo test --all-targets"}))
            .with_output("test result: ok")
            .with_output_summary("test result: ok")
            .with_duration_ms(5000),
    );
    let plain = strip_ansi(&output);
    assert!(plain.contains("cargo test --all-targets"));
    assert!(plain.contains("Took 5s"));
}

#[test]
fn a_subagent_started_in_the_background_shows_the_watch_hint_to_the_user_only() {
    let (renderer, mut events) = renderer_with_events();
    let finish = |renderer: &TerminalRenderer, is_error: bool, output: &str| {
        let mut line = ToolLine::new("subagent", serde_json::json!({"prompt": "audit the parser"}))
            .with_output(output)
            .with_output_summary(output);
        line.is_error = is_error;
        renderer.finish_tool_line(line);
    };
    let hint = "running in the background - press \u{2190} on an empty prompt to watch";

    finish(
        &renderer,
        false,
        "Started subagent task 'audit-1' (scout). Its result arrives as a follow-up message when it finishes.",
    );
    let (_, started) = drain_interactive_output(&mut events, &renderer.theme);
    finish(&renderer, false, "The parser is fine.");
    let (_, finished) = drain_interactive_output(&mut events, &renderer.theme);
    finish(&renderer, true, "Started subagent task 'x' failed");
    let (_, failed) = drain_interactive_output(&mut events, &renderer.theme);

    assert!(started.contains(hint), "{started}");
    assert!(!finished.contains(hint) && !failed.contains(hint));
}

#[test]
fn finished_read_block_includes_elapsed_duration() {
    let output = finished_output(
        ToolLine::new("read", serde_json::json!({"path": "src/main.rs"}))
            .with_output("hello world")
            .with_output_summary("hello world")
            .with_duration_ms(50),
    );
    assert!(output.contains("read") && output.contains("src/main.rs"));
    assert!(output.contains("Took 50ms"));
}

#[test]
fn finished_read_block_includes_line_range_styling() {
    let output = finished_output(ToolLine::new(
        "read",
        serde_json::json!({"path": "src/lib.rs", "offset": 10, "limit": 20}),
    ));
    assert!(output.contains("read") && output.contains("src/lib.rs") && output.contains(":10-29"));
}

#[test]
fn fetch_renders_url_on_same_line_without_duplicate() {
    let output = finished_output(
        ToolLine::new("web_fetch", serde_json::json!({"url": "https://serde.rs/"}))
            .with_output("serde docs")
            .with_output_summary("serde docs"),
    );
    assert!(output.contains("web_fetch"));
    assert!(output.contains("https://serde.rs/"));
    let plain = strip_ansi(&output);
    assert!(plain.contains("https://serde.rs/ (text)") && !plain.contains("fetched"));
    assert_eq!(output.matches("https://serde.rs/").count(), 1);
}

fn finished_view() -> CardView {
    CardView {
        status: CardStatus::Finished,
        expanded: false,
        show_read_expanded: false,
        dropped_lines: 0,
    }
}

#[test_case("subagent"; "subagent")]
#[test_case("workflow"; "workflow")]
fn finished_subagent_and_workflow_cards_hide_streamed_output(name: &str) {
    for theme in [Theme::default(), structured_theme()] {
        let output: String = (1..=8).map(|i| format!("step line {i}\n")).collect();
        let tool = ToolLine::new(name, serde_json::json!({"prompt": "step"}))
            .with_output(output)
            .with_duration_ms(10);
        let text = strip_ansi(&render_tool_block(&tool, 60, false, false, &theme));
        assert!(!text.contains("step line"), "{text}");
        assert!(!text.contains("earlier lines"), "{text}");
    }
}

#[test_case("bash", serde_json::json!({"command": "ls"}), 42, "Took 42ms"; "bash")]
#[test_case("rg", serde_json::json!({"pattern": "x"}), 1500, "Took 1.500s"; "rg")]
#[test_case("read", serde_json::json!({"path": "a.rs"}), 250, "Took 250ms"; "read")]
#[test_case("web_fetch", serde_json::json!({"url": "https://serde.rs/"}), 90, "Took 90ms"; "web fetch")]
#[test_case(
    "edit",
    serde_json::json!({"path": "a.rs", "edits": [{"oldText": "a", "newText": "b", "line": 1}]}),
    7,
    "Took 7ms";
    "edit"
)]
fn every_finished_tool_card_ends_with_a_took_line_on_its_own_line(
    name: &str,
    args: serde_json::Value,
    duration_ms: u64,
    expected: &str,
) {
    let theme = Theme::default();
    let dim = theme.dimmed;
    let tool = ToolLine::new(name, args).with_duration_ms(duration_ms);
    let mut content = String::new();
    crate::ui::render::card::append_tool_details(&mut content, &tool, 80, finished_view(), &theme);
    assert!(content.ends_with(&format!("\n\n{dim}{expected}{dim:#}")), "{content:?}");
    assert!(!format_tool_header(&tool, &theme).contains("Took"));
}

#[test]
fn untimed_tool_card_has_no_took_line() {
    let theme = Theme::default();
    let untimed = ToolLine::new("rg", serde_json::json!({"pattern": "x"}));
    let mut content = String::new();
    crate::ui::render::card::append_tool_details(&mut content, &untimed, 80, finished_view(), &theme);
    assert!(!content.contains("Took"));
}

#[test]
fn search_tool_displays_cleanly() {
    let output = finished_output(
        ToolLine::new("web_search", serde_json::json!({"query": "serde release"}))
            .with_output("results")
            .with_output_summary("results"),
    );
    assert!(output.contains("web_search"));
    assert!(output.contains("\"serde release\""));
}

// =========================================================================
// Notices & Presentation Tests
// =========================================================================

#[test]
fn print_session_status_and_notice_emit_transcript_item() {
    let (renderer, mut events) = renderer_with_events();

    renderer.print_session_status(&SessionStatus {
        model: "claude-sonnet".to_string(),
        provider: "anthropic".to_string(),
        context: "42% context".to_string(),
        quota: Some("80% quota".to_string()),
    });
    renderer.print_notice("  [Notice message]\n");

    let items = notice_texts(&mut events);

    assert_eq!(items.len(), 2);
    assert!(items[0].contains("claude-sonnet"));
    assert!(items[0].contains("42% context"));
    assert!(items[1].contains("[Notice message]"));
}

#[test]
fn print_welcome_emits_transcript_welcome_item() {
    let (renderer, mut events) = renderer_with_events();

    renderer.print_welcome(&WelcomeDisplay {
        model: "claude-sonnet".to_string(),
        provider: "anthropic".to_string(),
        resumed: true,
        agents: vec!["AGENTS.md".to_string()],
        tools: vec!["read".to_string()],
        skills: vec!["plan".to_string()],
        mcp: vec!["github".to_string()],
    });

    let item = std::iter::from_fn(|| events.try_recv().ok())
        .find_map(|event| match event {
            UiEvent::Transcript(TranscriptItem::Welcome(item)) => Some(item),
            _ => None,
        })
        .expect("welcome item");
    assert_eq!(item.version, rho_harness_core::build_info::welcome_version());
    assert_eq!(item.model, "claude-sonnet");
    assert_eq!(item.provider, "anthropic");
    assert!(item.resumed);
    assert_eq!(item.agents, ["AGENTS.md"]);
    assert_eq!(item.tools, ["read"]);
    assert_eq!(item.skills, ["plan"]);
    assert_eq!(item.mcp, ["github"]);
}

#[test]
fn print_block_emits_transcript_notice() {
    let (renderer, mut events) = renderer_with_events();

    renderer.print_block(&BlockDisplay {
        title: "Important Notice".to_string(),
        content: "Notice content goes here.".to_string(),
        style: "info".to_string(),
    });
    renderer.print_block(&BlockDisplay {
        title: "".to_string(),
        content: "Warning notice.".to_string(),
        style: "warning".to_string(),
    });

    let items = notice_texts(&mut events);

    assert_eq!(items.len(), 2);
    assert!(items[0].contains("Important Notice"));
    assert!(items[0].contains("Notice content"));
    assert!(items[1].contains("Warning notice."));
}

#[test]
fn render_block_content_styles_and_modes() {
    use crate::ui::render::notices::render_block_content;
    use crate::ui::theme::BlockStyle;

    let mut theme = Theme {
        block_style: BlockStyle::Border,
        ..Default::default()
    };

    let info_block = BlockDisplay {
        title: "Info Title".to_string(),
        content: "Info body".to_string(),
        style: "info".to_string(),
    };
    let rendered_border_info = render_block_content(&theme, &info_block);
    assert!(rendered_border_info.contains("Info Title"));
    assert!(rendered_border_info.contains("Info body"));

    let warn_block = BlockDisplay {
        title: "".to_string(),
        content: "Warn body".to_string(),
        style: "warning".to_string(),
    };
    let rendered_border_warn = render_block_content(&theme, &warn_block);
    assert!(rendered_border_warn.contains("Warn body"));

    theme.block_style = BlockStyle::Solid;
    let rendered_solid_info = render_block_content(&theme, &info_block);
    assert!(rendered_solid_info.contains("Info Title"));
    assert!(rendered_solid_info.contains("Info body"));

    let rendered_solid_warn = render_block_content(&theme, &warn_block);
    assert!(rendered_solid_warn.contains("Warn body"));
}

#[test]
fn print_block_without_ui_writes_output() {
    let renderer = TerminalRenderer::default();
    renderer.print_block(&BlockDisplay {
        title: "Direct".to_string(),
        content: "Console output".to_string(),
        style: "info".to_string(),
    });
}

#[test]
fn fetch_content_kind_uses_format_or_url_extension() {
    assert_eq!(
        fetch_content_kind(&serde_json::json!({"url": "https://example.com/page"})),
        "text"
    );
    assert_eq!(
        fetch_content_kind(&serde_json::json!({"url": "https://example.com/data.json"})),
        "json"
    );
    assert_eq!(
        fetch_content_kind(&serde_json::json!({"url": "https://example.com/file", "format": "pdf"})),
        "pdf"
    );
}

#[test]
fn bash_summary_formats_timeout_inline() {
    let with_timeout = format_tool_args_summary("bash", &serde_json::json!({"command": "cargo build", "timeout": 30}));
    assert_eq!(with_timeout, "cargo build (timeout 30s)");

    let without_timeout = format_tool_args_summary("bash", &serde_json::json!({"command": "cargo build"}));
    assert_eq!(without_timeout, "cargo build");
}

#[test]
fn format_bash_args_header_styles_timeout_part_as_dim() {
    let accent = anstyle::Style::new().bold();
    let dim = anstyle::Style::new().dimmed();
    let styled = format_bash_args_header("cargo build (timeout 30s)", accent, dim);
    assert_eq!(
        styled,
        format!("{accent}cargo build{accent:#} {dim}(timeout 30s){dim:#}")
    );

    let plain = format_bash_args_header("cargo build", accent, dim);
    assert_eq!(plain, format!("{accent}cargo build{accent:#}"));
}

#[test]
fn test_classify_read_path() {
    assert_eq!(
        classify_read_path(&serde_json::json!({"path": "/path/to/skills/plan/SKILL.md"})),
        Some(ReadClassification::Skill {
            name: "plan".to_string()
        })
    );
    assert_eq!(
        classify_read_path(&serde_json::json!({"path": "AGENTS.md"})),
        Some(ReadClassification::Resource {
            path: "AGENTS.md".to_string()
        })
    );
    assert_eq!(
        classify_read_path(&serde_json::json!({"path": "README.md"})),
        Some(ReadClassification::Docs {
            path: "README.md".to_string()
        })
    );
    assert_eq!(classify_read_path(&serde_json::json!({"path": "src/main.rs"})), None);
}

#[test]
fn read_summaries_show_explicit_line_ranges() {
    assert_eq!(
        read_summary_parts(&serde_json::json!({"path": "src/lib.rs", "offset": 10, "limit": 20})),
        ("src/lib.rs".to_string(), Some(":10-29".to_string()))
    );
    assert_eq!(
        read_summary_parts(&serde_json::json!({"path": "src/lib.rs"})),
        ("src/lib.rs".to_string(), None)
    );
}

#[test]
fn test_to_relative_path() {
    let cwd = std::env::current_dir().unwrap();
    let abs = cwd.join("src/main.rs");
    let rel = to_relative_path(abs.to_str().unwrap());
    assert_eq!(rel, "src/main.rs");
}

#[test]
fn test_clean_command_paths() {
    let cwd = std::env::current_dir().unwrap();
    let cwd_str = cwd.to_str().unwrap();
    let cmd = format!("cat {cwd_str}/Cargo.toml");
    let cleaned = clean_command_paths(&cmd);
    assert_eq!(cleaned, "cat Cargo.toml");
}

#[test]
fn notice_kinds_get_label_prefix_and_spacing() {
    let theme = Theme::default();
    let cases = [
        (NoticeKind::Info, "info", theme.highlight),
        (NoticeKind::Warn, "warning", theme.warning),
        (NoticeKind::Error, "error", theme.tool_err),
    ];
    for (kind, label, style) in cases {
        let notice = format_notice(&theme, kind, "\nsomething happened\n");
        let style = style.bold();
        assert_eq!(notice, format!("\n {style}{label}{style:#} something happened\n"));
    }
    let multi = strip_ansi(&format_notice(&theme, NoticeKind::Warn, "first\nsecond"));
    assert_eq!(multi, "\n warning first\n second\n");
}

#[test]
fn renderer_notice_is_emitted_as_a_transcript_notice() {
    let (renderer, mut events) = renderer_with_events();
    renderer.notice(NoticeKind::Warn, "Could not switch model: boom");
    let Ok(UiEvent::Transcript(TranscriptItem::Notice(text))) = events.try_recv() else {
        panic!("expected a notice transcript item");
    };
    assert!(strip_ansi(&text).contains("warning Could not switch model: boom"));
}

fn write_preview(args: &serde_json::Value, theme: &Theme, expanded: bool, width: usize) -> Option<String> {
    let request = WritePreview {
        args,
        dropped_lines: 0,
        expanded,
        width,
    };
    format_write_preview(request, theme)
}

fn numbered(prefix: &str, count: usize) -> String {
    (1..=count)
        .map(|n| format!("{prefix}{n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn multi_edit_diffs_label_every_edit_including_the_first() {
    let theme = Theme::default();
    let two = serde_json::json!({"edits": [
        {"oldText": "a", "newText": "b", "line": 3},
        {"oldText": "c", "newText": "d"},
    ]});
    let plain = strip_ansi(&edit_diff(&two, &theme).unwrap());
    assert!(plain.starts_with("@@ edit #1/2 \u{b7} line 3 @@\n"), "{plain}");
    assert!(plain.contains("@@ edit #2/2 @@\n"));

    let one = serde_json::json!({"edits": [{"oldText": "a", "newText": "b", "line": 3}]});
    assert!(!strip_ansi(&edit_diff(&one, &theme).unwrap()).contains("@@"));
}

#[test]
fn edit_diff_collapses_to_its_newest_rows_until_expanded() {
    let theme = Theme::default();
    let args = serde_json::json!({"edits": [{"oldText": numbered("old", 12), "newText": "new"}]});
    let request = |expanded| EditDiffRequest {
        args: &args,
        file_change: None,
        expanded,
        width: 60,
    };
    let collapsed = strip_ansi(&format_edit_diff(request(false), &theme).unwrap());
    assert_eq!(
        collapsed.lines().next(),
        Some("... (3 earlier lines, 13 total, Ctrl+O to expand)")
    );
    assert_eq!(collapsed.lines().count(), 11);
    assert!(collapsed.contains("- old12") && collapsed.contains("+ new") && !collapsed.contains("old3\n"));
    let expanded = strip_ansi(&format_edit_diff(request(true), &theme).unwrap());
    assert!(expanded.contains("- old1\n") && !expanded.contains("earlier lines"));
}

#[test]
fn collapsed_edit_diff_height_is_fixed_and_rows_are_clipped() {
    let theme = Theme::default();
    let wide = "x".repeat(200);
    let heights: Vec<usize> = [1usize, 5, 9, 30, 60]
        .iter()
        .map(|&n| {
            let args = serde_json::json!({"edits": [{"oldText": numbered(&wide, n), "newText": "y"}]});
            let request = EditDiffRequest {
                args: &args,
                file_change: None,
                expanded: false,
                width: 40,
            };
            let plain = strip_ansi(&format_edit_diff(request, &theme).unwrap());
            assert!(plain.lines().all(|row| row.chars().count() <= 40), "{plain}");
            plain.lines().count()
        })
        .collect();
    assert_eq!(heights, [2, 6, 10, 11, 11]);
}

#[test]
fn edit_diff_prefers_recorded_start_lines_over_the_file_on_disk() {
    let theme = Theme::default();
    let args = serde_json::json!({"path": "/nonexistent/never.rs", "edits": [{"oldText": "a", "newText": "b"}]});
    let change = FileChange {
        overwrote_existing: true,
        edit_start_lines: vec![Some(41)],
        replaced_regions: vec![],
    };
    let request = EditDiffRequest {
        args: &args,
        file_change: Some(&change),
        expanded: false,
        width: 60,
    };
    let plain = strip_ansi(&format_edit_diff(request, &theme).unwrap());
    assert!(
        plain.contains(" 41 \u{2502} - a") && plain.contains(" 41 \u{2502} + b"),
        "{plain}"
    );
}

#[test]
fn tabs_expand_to_four_spaces_in_diffs_and_write_previews() {
    let theme = Theme::default();
    let args = serde_json::json!({"edits": [{"oldText": "\tx\ty", "newText": "\tz"}]});
    let diff = strip_ansi(&edit_diff(&args, &theme).unwrap());
    assert!(!diff.contains('\t') && diff.contains("-     x"), "{diff:?}");
    let write = serde_json::json!({"path": "a.txt", "content": "\tindented"});
    let preview = strip_ansi(&write_preview(&write, &theme, true, 60).unwrap());
    assert!(preview.contains("\u{2502}     indented"), "{preview:?}");
}

#[test]
fn every_edit_and_write_card_collapses_to_its_newest_rows_until_expanded() {
    let body = numbered("row", 30);
    let edit = serde_json::json!({"path": "a.rs", "edits": [{"oldText": body, "newText": "done", "line": 1}]});
    let write = serde_json::json!({"path": "a.rs", "text": format!("{body}\ndone")});
    let structured = structured_theme();
    let cards = [
        ("edit", ToolLine::new("edit", edit.clone())),
        (
            "failed edit",
            ToolLine::new("edit", edit).with_output("no match").failed(),
        ),
        ("write", ToolLine::new("write", write)),
    ];
    for theme in [Theme::default(), structured] {
        for (label, tool) in &cards {
            let collapsed = strip_ansi(&render_tool_block(tool, 80, false, false, &theme));
            assert!(
                collapsed.contains("earlier lines") && collapsed.contains("done") && !collapsed.contains("row1 "),
                "{label}: {collapsed}"
            );
            let expanded = strip_ansi(&render_tool_block(tool, 80, true, false, &theme));
            assert!(
                !expanded.contains("earlier lines") && expanded.contains("row1 "),
                "{label}: {expanded}"
            );
        }
    }
}

#[test]
fn failed_edit_card_shows_the_attempted_change_after_the_error() {
    let theme = Theme::default();
    let args = serde_json::json!({"path": "a.rs", "edits": [{"oldText": "missing", "newText": "present", "line": 7}]});
    let failed = ToolLine::new("edit", args.clone()).with_output("no match").failed();
    let card = strip_ansi(&render_tool_transcript(&failed, 80, false, false, &theme));
    let (error_at, attempt_at) = (card.find("no match").unwrap(), card.find("Attempted change:").unwrap());
    assert!(
        error_at < attempt_at && card.contains("- missing") && card.contains("+ present"),
        "{card}"
    );

    let ok = strip_ansi(&render_tool_transcript(
        &ToolLine::new("edit", args),
        80,
        false,
        false,
        &theme,
    ));
    assert!(!ok.contains("Attempted change:"));
}

#[test]
fn edit_and_write_cards_show_change_summary_in_the_header() {
    let theme = Theme::default();
    let edit = serde_json::json!({"path": "a.rs", "edits": [{"oldText": "a\nb", "newText": "c", "line": 1}]});
    let write = serde_json::json!({"path": "a.txt", "content": "x\ny"});
    let new_file = FileChange {
        overwrote_existing: false,
        edit_start_lines: vec![],
        replaced_regions: vec![],
    };
    let edit_header = strip_ansi(&format_tool_header(&ToolLine::new("edit", edit), &theme));
    let write_header = strip_ansi(&format_tool_header(
        &ToolLine::new("write", write).with_file_change(new_file),
        &theme,
    ));
    assert!(edit_header.ends_with("\u{b7} 1 edit \u{b7} +1 -2"), "{edit_header}");
    assert!(
        write_header.ends_with("\u{b7} 2 lines \u{b7} 3B \u{b7} new file"),
        "{write_header}"
    );
}

#[test]
fn a_finished_wait_ends_the_running_tool_without_a_card() {
    let (renderer, mut events) = renderer_with_events();

    renderer.start_tool_run("wait", &serde_json::json!({}));
    renderer.finish_tool_line(ToolLine::new("wait", serde_json::json!({})));

    let events = drain_events(&mut events);
    assert!(
        !events.iter().any(|event| matches!(event, UiEvent::Transcript(_))),
        "wait leaves no transcript card"
    );
    assert!(matches!(events.last(), Some(UiEvent::ToolEnd)));
}

#[test]
fn renderer_gives_the_live_card_and_the_finished_line_the_snapshot_taken_when_the_tool_started() {
    let dir = rho_test_support::TestDir::new();
    let file = dir.join("f.txt");
    std::fs::write(&file, "keep\nchange me\n").unwrap();
    let path = file.to_str().unwrap();
    let (renderer, mut events) = renderer_with_events();
    let args = serde_json::json!({"path": path, "edits": [{"oldText": "change me", "newText": "changed"}]});

    renderer.start_tool_run("edit", &args);
    std::fs::write(&file, "keep\nchanged\nextra\n").unwrap();
    renderer.finish_tool_line(ToolLine::new("edit", args));

    let events = drain_events(&mut events);
    let [
        UiEvent::ToolStart(start),
        UiEvent::Transcript(TranscriptItem::Tool(line)),
    ] = &events[..]
    else {
        panic!("an edit opens a live card, then finishes into the transcript");
    };
    let change = line
        .file_change
        .as_ref()
        .expect("the line carries the pre-run snapshot");
    assert_eq!(change.edit_start_lines, [Some(2)]);
    assert_eq!(start.file_change.as_ref(), Some(change), "the live card reads the same");
    assert!(renderer.pending_file_changes.is_empty());
}
