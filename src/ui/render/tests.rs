//! Tests for the `ui::render` module.

use super::formatters::{format_edit_diff, format_session_status, format_thinking_block, format_write_preview};
use super::preview::{fetch_content_kind, tool_title_style};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InteractiveUi, OutputEvent, UiEvent};
use crate::ui::theme::Theme;
use rho_harness_core::presentation::summary::{clean_command_paths, read_summary_parts, to_relative_path};
use rho_harness_core::presentation::{SessionStatus, ToolLine};

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

    let mut activity_events = Vec::new();
    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Activity(activity) => activity_events.push(activity),
            UiEvent::Transcript(item) => output.push_str(&crate::ui::interactive::render_transcript_item(
                crate::ui::interactive::TranscriptRenderInput {
                    item: &item,
                    theme: &renderer.theme,
                    width: 80,
                    tools_expanded: false,
                    hide_thinking: false,
                },
            )),
            UiEvent::RunningTool(_)
            | UiEvent::ExtraStatus(_)
            | UiEvent::ToolStart(_)
            | UiEvent::ToolChunk { .. }
            | UiEvent::ToolEnd => {}
            UiEvent::Output(OutputEvent::Text(text)) => output.push_str(&text),
            UiEvent::Interaction { .. } => panic!("unexpected interaction"),
        }
    }
    assert_eq!(activity_events, [Activity::Thinking, Activity::Idle]);
    assert!(output.contains("considering"));
    assert!(output.contains("answer"));
    assert!(output.contains("read"));
    assert!(output.contains("src/lib.rs"));
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

    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Transcript(item) => output.push_str(&crate::ui::interactive::render_transcript_item(
                crate::ui::interactive::TranscriptRenderInput {
                    item: &item,
                    theme: &renderer.theme,
                    width: 80,
                    tools_expanded: false,
                    hide_thinking: false,
                },
            )),
            UiEvent::Output(OutputEvent::Text(text)) => output.push_str(&text),
            UiEvent::Activity(_)
            | UiEvent::RunningTool(_)
            | UiEvent::ExtraStatus(_)
            | UiEvent::ToolStart(_)
            | UiEvent::ToolChunk { .. }
            | UiEvent::ToolEnd => {}
            UiEvent::Interaction { .. } => panic!("unexpected interaction"),
        }
    }
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

    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Transcript(item) => output.push_str(&crate::ui::interactive::render_transcript_item(
                crate::ui::interactive::TranscriptRenderInput {
                    item: &item,
                    theme: &renderer.theme,
                    width: 80,
                    tools_expanded: false,
                    hide_thinking: false,
                },
            )),
            UiEvent::Output(OutputEvent::Text(text)) => output.push_str(&text),
            UiEvent::Activity(_)
            | UiEvent::RunningTool(_)
            | UiEvent::ExtraStatus(_)
            | UiEvent::ToolStart(_)
            | UiEvent::ToolChunk { .. }
            | UiEvent::ToolEnd => {}
            UiEvent::Interaction { .. } => panic!("unexpected interaction"),
        }
    }
    assert!(output.contains("read"));
    assert!(output.contains("src/main.rs"));
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

    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        if let UiEvent::Transcript(item) = event {
            output.push_str(&crate::ui::interactive::render_transcript_item(
                crate::ui::interactive::TranscriptRenderInput {
                    item: &item,
                    theme: &renderer.theme,
                    width: 80,
                    tools_expanded: false,
                    hide_thinking: false,
                },
            ));
        }
    }
    assert!(output.contains("read"));
    assert!(output.contains("src/lib.rs"));
    assert!(output.contains(":10-29"));
}

#[test]
fn bash_summary_formats_timeout_inline() {
    use rho_harness_core::presentation::summary::format_tool_args_summary;
    let with_timeout = format_tool_args_summary("bash", &serde_json::json!({"command": "cargo build", "timeout": 30}));
    assert_eq!(with_timeout, "`cargo build` (timeout 30s)");

    let without_timeout = format_tool_args_summary("bash", &serde_json::json!({"command": "cargo build"}));
    assert_eq!(without_timeout, "`cargo build`");
}

#[test]
fn fetch_renders_url_on_same_line_without_duplicate() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.finish_tool_line(ToolLine {
        name: "fetch".to_string(),
        arguments: serde_json::json!({"url": "https://serde.rs/"}),
        is_error: false,
        output: "serde docs".to_string(),
        output_summary: "serde docs".to_string(),
        duration_ms: None,
    });

    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Transcript(item) => output.push_str(&crate::ui::interactive::render_transcript_item(
                crate::ui::interactive::TranscriptRenderInput {
                    item: &item,
                    theme: &renderer.theme,
                    width: 80,
                    tools_expanded: false,
                    hide_thinking: false,
                },
            )),
            UiEvent::Output(OutputEvent::Text(text)) => output.push_str(&text),
            _ => {}
        }
    }
    assert!(output.contains("fetch"));
    assert!(output.contains("https://serde.rs/"));
    assert!(output.contains("fetched (text)"));
    assert_eq!(output.matches("https://serde.rs/").count(), 1);
}

#[test]
fn search_tool_displays_cleanly() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.finish_tool_line(ToolLine {
        name: "search".to_string(),
        arguments: serde_json::json!({"query": "serde release"}),
        is_error: false,
        output: "results".to_string(),
        output_summary: "results".to_string(),
        duration_ms: None,
    });

    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Transcript(item) => output.push_str(&crate::ui::interactive::render_transcript_item(
                crate::ui::interactive::TranscriptRenderInput {
                    item: &item,
                    theme: &renderer.theme,
                    width: 80,
                    tools_expanded: false,
                    hide_thinking: false,
                },
            )),
            UiEvent::Output(OutputEvent::Text(text)) => output.push_str(&text),
            _ => {}
        }
    }
    assert!(output.contains("search"));
    assert!(output.contains("\"serde release\""));
}

#[test]
fn print_session_status_and_notice_emit_transcript_item() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.print_session_status(&SessionStatus {
        model: "claude-sonnet".to_string(),
        provider: "anthropic".to_string(),
        context: "42% context".to_string(),
        quota: Some("80% quota".to_string()),
        auto_approve: true,
    });
    renderer.print_notice("  [Notice message]\n");

    let items = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Notice(text)) => Some(text),
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
    renderer.print_cache_miss_notice(crate::ui::render::renderer::CacheMissNotice {
        missed_tokens: 45_000,
        cost: Some(0.14),
        idle_minutes: Some(5),
    });

    let items = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Notice(text)) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(items.len(), 2);
    assert!(items[0].contains("Compaction: 154k tokens billed (~$0.46)"));
    assert!(items[1].contains("Cache miss after 5m idle: 45k tokens re-billed (~$0.14)"));
}

#[test]
fn test_classify_read_path() {
    use rho_harness_core::presentation::summary::{ReadClassification, classify_read_path};

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
    assert!(diff.contains("```diff"));
    assert!(diff.contains("- let x = 1;"));
    assert!(diff.contains("+ let x = 2;"));
    assert!(diff.contains("+ let y = 3;"));
    assert!(diff.contains("```"));
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
    assert!(diff.contains("```diff"));
    // Verify unchanged leading indentation is preserved without inverse escape
    assert!(diff.contains("-     let "));
    assert!(diff.contains("+     let "));
    // Verify modified tokens are wrapped in inverse video \x1b[7m ... \x1b[27m
    assert!(diff.contains("\x1b[7mold_val\x1b[27m"));
    assert!(diff.contains("\x1b[7mnew_val\x1b[27m"));
    // Verify trailing unchanged token is preserved
    assert!(diff.contains(" = 10;"));
}

#[test]
fn test_format_write_preview_renders_additions() {
    let theme = Theme::default();
    let args = serde_json::json!({
        "path": "test.py",
        "content": "def main():\n    print('hello')"
    });
    let preview = format_write_preview(&args, &theme, false).unwrap();
    assert!(preview.contains("```diff"));
    assert!(preview.contains("+ def main():"));
    assert!(preview.contains("+     print('hello')"));
    assert!(preview.contains("```"));

    let long_content = (1..=12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let long_args = serde_json::json!({
        "path": "test.txt",
        "content": long_content
    });
    let collapsed = format_write_preview(&long_args, &theme, false).unwrap();
    assert!(collapsed.contains("... (4 more lines, 12 total, Ctrl+O to expand)"));
    assert!(!collapsed.contains("+ line 12"));

    let expanded = format_write_preview(&long_args, &theme, true).unwrap();
    assert!(!expanded.contains("Ctrl+O to expand"));
    assert!(expanded.contains("+ line 12"));
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
            auto_approve: false,
        }),
        "claude-sonnet | 27.4% (1M) | 93% (3h22m)"
    );
    assert_eq!(
        format_session_status(&SessionStatus {
            model: "qwen".to_string(),
            provider: "ollama".to_string(),
            context: "0% (376k)".to_string(),
            quota: None,
            auto_approve: true,
        }),
        "qwen | 0% (376k)"
    );
}

#[test]
fn test_format_thinking_block_renders_dimmed_with_trailing_breaks() {
    let theme = Theme::default();
    let formatted = format_thinking_block("analyzing the problem\nchecking tests", &theme);
    assert!(formatted.contains("analyzing the problem"));
    assert!(formatted.contains("checking tests"));
    assert!(!formatted.contains("┌─ Thinking"));
    assert!(formatted.ends_with("\n\n"));
}
