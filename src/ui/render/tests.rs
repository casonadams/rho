use super::CacheMissNotice;
use super::card::{fetch_content_kind, format_bash_args_header, tool_title_style};
use super::diff::format_gutter_prefix;
use super::formatters::{
    format_edit_diff, format_read_expanded, format_relative_time, format_session_status, format_thinking_block,
    format_write_preview,
};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{
    Activity, InteractiveUi, OutputEvent, TranscriptItem, TranscriptRenderInput, UiEvent, render_transcript_item,
};
use crate::ui::theme::Theme;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rho_harness_core::presentation::summary::{
    ReadClassification, classify_read_path, clean_command_paths, format_tool_args_summary, read_summary_parts,
    to_relative_path,
};
use rho_harness_core::presentation::{BlockDisplay, SessionStatus, ToolLine};

fn assert_contains_all(s: &str, items: &[&str]) {
    for item in items {
        assert!(s.contains(item));
    }
}

// =========================================================================
// Formatters Tests
// =========================================================================

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
    let diff = format_edit_diff(&args, &theme).unwrap();
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
    let diff = format_edit_diff(&args, &theme).unwrap();
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
    let preview = format_write_preview(&args, &theme, false).unwrap();
    assert!(!preview.contains("```diff") && !preview.contains("```") && !preview.contains("+ def main():"));
    assert_contains_all(&preview, &["def", "main", "print", "hello", "  1 │ ", "  2 │ "]);
}

#[test]
fn test_format_write_preview_expansion_and_collapse() {
    let theme = Theme::default();
    let long_content = (1..=12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let long_args = serde_json::json!({
        "path": "test.txt",
        "content": long_content
    });
    let collapsed = format_write_preview(&long_args, &theme, false).unwrap();
    assert!(collapsed.contains("... (4 more lines, 12 total)") && !collapsed.contains("line 12"));

    let expanded = format_write_preview(&long_args, &theme, true).unwrap();
    assert!(!expanded.contains("more lines") && expanded.contains("line 12"));
}

#[test]
fn test_format_thinking_block_renders_dimmed_with_trailing_breaks() {
    let theme = Theme::default();
    let formatted = format_thinking_block("analyzing the problem\nchecking tests", &theme, 80);
    assert!(formatted.contains(" analyzing the problem"));
    assert!(formatted.contains(" checking tests"));
    assert!(!formatted.contains("┌─ Thinking"));
    assert!(formatted.ends_with('\n'));
}

#[test]
fn test_format_thinking_block_wraps_on_word_boundaries() {
    let theme = Theme::default();
    let text = "first second third fourth fifth";
    let formatted = format_thinking_block(text, &theme, 15);
    let lines: Vec<&str> = formatted.trim().lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains("first second"));
    assert!(lines[1].contains("third fourth"));
    assert!(lines[2].contains("fifth"));
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
    let diff = format_edit_diff(&args, &theme).unwrap();
    assert!(diff.contains(" 42 │ "));
    assert!(diff.contains(" 43 │ "));
    assert!(diff.contains("- let a = 1;"));
    assert!(diff.contains("+ let a = 10;"));
}

#[test]
fn test_format_edit_diff_locates_line_from_file_on_disk() {
    let theme = Theme::default();
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("example.rs");
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

    let diff_before = format_edit_diff(&args, &theme).unwrap();
    assert_contains_all(&diff_before, &["  4 │ ", "target", "replaced"]);

    std::fs::write(&file_path, "line 1\nline 2\nline 3\nreplaced line\nline 5\n").unwrap();
    let diff_after = format_edit_diff(&args, &theme).unwrap();
    assert_contains_all(&diff_after, &["  4 │ ", "target", "replaced"]);
}

#[test]
fn test_format_edit_diff_multi_edit_resolves_lines_from_single_read() {
    let theme = Theme::default();
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("example_multi.rs");
    std::fs::write(&file_path, "fn one() {}\n\nfn two() {}\n").unwrap();

    let path_str = file_path.to_str().unwrap();
    let args = serde_json::json!({
        "path": path_str,
        "edits": [
            { "oldText": "fn one() {}", "newText": "fn first() {}" },
            { "oldText": "fn two() {}", "newText": "fn second() {}" }
        ]
    });

    let diff = format_edit_diff(&args, &theme).unwrap();
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

#[test]
fn test_format_read_expanded_standard_numbered_lines() {
    let theme = Theme::default();
    let args = serde_json::json!({ "path": "src/main.rs" });
    let raw = "     1\tfn main() {\n     2\t    println!(\"hello\");\n     3\t}\n";
    let formatted = format_read_expanded(raw, &args, &theme).unwrap();
    assert_contains_all(&formatted, &["  1 │ ", "  2 │ ", "  3 │ ", "main", "println"]);
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
fn test_format_read_expanded_with_offset() {
    let theme = Theme::default();
    let args = serde_json::json!({ "path": "src/main.rs", "offset": 100 });
    let raw = "   100\tlet a = 1;\n   101\tlet b = 2;\n";
    let formatted = format_read_expanded(raw, &args, &theme).unwrap();
    assert_contains_all(&formatted, &["100 │ ", "101 │ "]);
}

#[test]
fn test_format_read_expanded_unlabelled_fallback() {
    let theme = Theme::default();
    let args = serde_json::json!({ "path": "src/main.rs" });
    let raw = "fn main() {\n    println!(\"hello\");\n}\n";
    let formatted = format_read_expanded(raw, &args, &theme).unwrap();
    assert_contains_all(&formatted, &["  1 │ ", "  2 │ ", "  3 │ "]);
}

#[test]
fn test_line_number_gutter_parity_across_read_edit_write() {
    let theme = Theme::default();

    let write_args = serde_json::json!({ "path": "example.rs", "content": "fn test() {}" });
    let write_out = format_write_preview(&write_args, &theme, false).unwrap();

    let edit_args = serde_json::json!({
        "path": "example.rs",
        "edits": [{ "oldText": "fn old() {}", "newText": "fn test() {}", "line": 1 }]
    });
    let edit_out = format_edit_diff(&edit_args, &theme).unwrap();

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

#[test]
fn session_status_joins_model_context_and_optional_quota() {
    let cases = [
        ("claude-sonnet", "27.4% (1M)", None, "claude-sonnet | 27.4% (1M)"),
        (
            "claude-sonnet",
            "27.4% (1M)",
            Some("93% (3h22m)"),
            "claude-sonnet | 27.4% (1M) | 93% (3h22m)",
        ),
        (
            "qwen-日本語モデル",
            "0% (376k)",
            Some("80% quota"),
            "qwen-日本語モデル | 0% (376k) | 80% quota",
        ),
    ];
    for (model, context, quota, expected) in cases {
        let session = SessionStatus {
            model: model.to_string(),
            provider: "test".to_string(),
            context: context.to_string(),
            quota: quota.map(str::to_string),
        };
        assert_eq!(format_session_status(&session), expected);
    }
}

// =========================================================================
// Interactive Renderer Tests
// =========================================================================

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
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.print_token("answer");

    assert!(matches!(
        events.try_recv(),
        Ok(UiEvent::Output(OutputEvent::StreamText(text))) if text == "answer"
    ));
}

#[test]
fn interactive_renderer_emits_formatted_output_and_activity_events() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    let activity = renderer.start_spinner("thinking...");
    renderer.print_thinking_token("considering");
    activity.finish_and_clear();
    renderer.print_token("answer");
    renderer.flush();
    renderer.finish_tool_line(ToolLine {
        name: "read".to_string(),
        arguments: serde_json::json!({"path": "src/lib.rs"}),
        is_error: false,
        output: "contents".to_string(),
        output_summary: "contents".to_string(),
        duration_ms: None,
    });

    let (activity_events, output) = drain_interactive_output(&mut events, &renderer.theme);
    assert_eq!(activity_events, [Activity::Thinking, Activity::Idle]);
    for token in ["considering", "answer", "read", "src/lib.rs"] {
        assert!(output.contains(token));
    }
}

#[test]
fn renderer_flush_resets_markdown_state_between_turns() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

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
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    assert_fast_tool_run(
        &renderer,
        &mut events,
        "edit",
        &serde_json::json!({"path": "src/main.rs"}),
    );
    assert_fast_tool_run(
        &renderer,
        &mut events,
        "write",
        &serde_json::json!({"path": "src/main.rs", "content": "..."}),
    );
    assert_fast_tool_run(
        &renderer,
        &mut events,
        "read",
        &serde_json::json!({"path": "src/main.rs"}),
    );
}

#[test]
fn finished_bash_block_includes_elapsed_duration() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.finish_tool_line(ToolLine {
        name: "bash".to_string(),
        arguments: serde_json::json!({"command": "cargo test --all-targets"}),
        is_error: false,
        output: "test result: ok".to_string(),
        output_summary: "test result: ok".to_string(),
        duration_ms: Some(5000),
    });

    let (_, output) = drain_interactive_output(&mut events, &renderer.theme);
    assert!(output.contains("cargo test --all-targets"));
    assert!(output.contains("Took 5s"));
}

#[test]
fn finished_read_block_omits_elapsed_duration() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.finish_tool_line(ToolLine {
        name: "read".to_string(),
        arguments: serde_json::json!({"path": "src/main.rs"}),
        is_error: false,
        output: "hello world".to_string(),
        output_summary: "hello world".to_string(),
        duration_ms: Some(50),
    });

    let (_, output) = drain_interactive_output(&mut events, &renderer.theme);
    assert!(output.contains("read") && output.contains("src/main.rs"));
    assert!(!output.contains("Took"));
}

#[test]
fn finished_read_block_includes_line_range_styling() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.finish_tool_line(ToolLine {
        name: "read".to_string(),
        arguments: serde_json::json!({"path": "src/lib.rs", "offset": 10, "limit": 20}),
        is_error: false,
        output: "".to_string(),
        output_summary: "".to_string(),
        duration_ms: None,
    });

    let (_, output) = drain_interactive_output(&mut events, &renderer.theme);
    assert!(output.contains("read") && output.contains("src/lib.rs") && output.contains(":10-29"));
}

#[test]
fn fetch_renders_url_on_same_line_without_duplicate() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.finish_tool_line(ToolLine {
        name: "web_fetch".to_string(),
        arguments: serde_json::json!({"url": "https://serde.rs/"}),
        is_error: false,
        output: "serde docs".to_string(),
        output_summary: "serde docs".to_string(),
        duration_ms: None,
    });

    let (_, output) = drain_interactive_output(&mut events, &renderer.theme);
    assert!(output.contains("web_fetch"));
    assert!(output.contains("https://serde.rs/"));
    assert!(output.contains("fetched (text)"));
    assert_eq!(output.matches("https://serde.rs/").count(), 1);
}

#[test]
fn search_tool_displays_cleanly() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.finish_tool_line(ToolLine {
        name: "web_search".to_string(),
        arguments: serde_json::json!({"query": "serde release"}),
        is_error: false,
        output: "results".to_string(),
        output_summary: "results".to_string(),
        duration_ms: None,
    });

    let (_, output) = drain_interactive_output(&mut events, &renderer.theme);
    assert!(output.contains("web_search"));
    assert!(output.contains("\"serde release\""));
}

// =========================================================================
// Notices & Presentation Tests
// =========================================================================

#[test]
fn print_session_status_and_notice_emit_transcript_item() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.print_session_status(&SessionStatus {
        model: "claude-sonnet".to_string(),
        provider: "anthropic".to_string(),
        context: "42% context".to_string(),
        quota: Some("80% quota".to_string()),
    });
    renderer.print_notice("  [Notice message]\n");

    let items = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::Transcript(TranscriptItem::Notice(text)) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(items.len(), 2);
    assert!(items[0].contains("claude-sonnet"));
    assert!(items[0].contains("42% context"));
    assert!(items[1].contains("[Notice message]"));
}

#[test]
fn print_compaction_and_cache_miss_notices() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.print_compaction_cost_notice(154_000, Some(0.46));
    renderer.print_cache_miss_notice(CacheMissNotice {
        missed_tokens: 45_000,
        cost: Some(0.14),
        idle_minutes: Some(5),
    });

    let items = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::Transcript(TranscriptItem::Notice(text)) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(items.len(), 2);
    assert!(items[0].contains("Compaction: 154k tokens billed (~$0.46)"));
    assert!(items[1].contains("Cache miss after 5m idle: 45k tokens re-billed (~$0.14)"));
}

#[test]
fn print_block_emits_transcript_notice() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.print_block(&BlockDisplay {
        title: "Important Notice".to_string(),
        content: "Detailed content goes here.".to_string(),
        style: "info".to_string(),
    });
    renderer.print_block(&BlockDisplay {
        title: "".to_string(),
        content: "Warning without title.".to_string(),
        style: "warning".to_string(),
    });

    let items = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::Transcript(TranscriptItem::Notice(text)) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(items.len(), 2);
    assert!(items[0].contains("Important Notice"));
    assert!(items[0].contains("Detailed content goes here."));
    assert!(items[1].contains("Warning without title."));
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
fn error_tool_titles_use_terminal_red_without_dimming() {
    assert_eq!(tool_title_style(false).render().to_string(), "\x1b[1m");
    assert_eq!(tool_title_style(true).render().to_string(), "\x1b[1m\x1b[31m");
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
fn session_status_keeps_runtime_context_visible() {
    assert_eq!(
        format_session_status(&SessionStatus {
            model: "claude-sonnet".to_string(),
            provider: "anthropic".to_string(),
            context: "27.4% (1M)".to_string(),
            quota: Some("93% (3h22m)".to_string()),
        }),
        "claude-sonnet | 27.4% (1M) | 93% (3h22m)"
    );
    assert_eq!(
        format_session_status(&SessionStatus {
            model: "qwen".to_string(),
            provider: "ollama".to_string(),
            context: "0% (376k)".to_string(),
            quota: None,
        }),
        "qwen | 0% (376k)"
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
