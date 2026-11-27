use super::super::helpers::{drain_ui_events, presenter, terminal_session};
use crate::engine::runner::{TerminalApprovalSink, TerminalSinkConfig};
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InteractiveUi, OutputEvent, UiEvent};

fn collect_output_events(events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>) -> String {
    drain_ui_events(events, |event| match event {
        UiEvent::Output(OutputEvent::Text(text) | OutputEvent::StreamText(text)) => Some(text),
        _ => None,
    })
    .concat()
}

#[test]
fn thinking_trailing_newlines_collapse_to_single_blank_line_before_text() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let (session, _dir) = terminal_session();
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
    );

    sink.emit_reasoning("Analyzing the code.\n\n\n");
    sink.emit_text("Here is the summary.");

    let output = collect_output_events(&mut events);

    assert!(
        !output.contains("\n\n\n"),
        "Output contained excess newlines: {output:?}"
    );
    assert!(output.contains("\n\n Here is the summary."));
}

#[test]
fn token_by_token_reasoning_with_trailing_newlines_collapses_cleanly() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let (session, _dir) = terminal_session();
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
    );

    sink.emit_reasoning("Analyzing");
    sink.emit_reasoning(" step 1.");
    sink.emit_reasoning("\n\n\n");
    sink.emit_text("Summary.");

    let output = collect_output_events(&mut events);
    assert!(
        !output.contains("\n\n\n"),
        "Output contained excess newlines: {output:?}"
    );
    assert!(output.contains("\n\n Summary."));
}

#[test]
fn internal_reasoning_paragraphs_preserve_single_blank_line() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let (session, _dir) = terminal_session();
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
    );

    sink.emit_reasoning("Paragraph 1.\n\n\n\n");
    sink.emit_reasoning("Paragraph 2.\n\n\n");
    sink.emit_text("Answer.");

    let output = collect_output_events(&mut events);
    assert!(
        !output.contains("\n\n\n"),
        "Output contained excess newlines: {output:?}"
    );
    let clean = crate::ui::block::strip_ansi(&output);
    assert!(clean.contains("Paragraph 1."));
    assert!(clean.contains("Paragraph 2."));
    assert!(output.contains("\n\n Answer."));
}

#[test]
fn thinking_followed_by_tool_closes_line_cleanly() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let (session, _dir) = terminal_session();
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
    );

    sink.emit_reasoning("Let me read the file.\n\n\n");
    sink.tool_start("read", &serde_json::json!({ "path": "src/lib.rs" }));

    let output = collect_output_events(&mut events);
    assert!(
        !output.contains("\n\n\n"),
        "Output contained excess newlines: {output:?}"
    );
    assert!(output.ends_with('\n'), "Output should end with newline: {output:?}");
}

#[test]
fn thinking_followed_by_skipped_tool_flushes_reasoning_and_closes_line() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);
    let (session, _dir) = terminal_session();
    let sink = TerminalApprovalSink::new(
        &presenter(&renderer),
        TerminalSinkConfig {
            model_label: "model".to_string(),
            run_tracker: crate::engine::metrics::RunTracker::default(),
        },
        session,
    );

    sink.emit_reasoning("Let me read the file.\n\n\n");
    sink.tool_finished(crate::engine::runner::ToolFinishDetails {
        name: "bash",
        arguments: &serde_json::json!({ "command": "rm -rf *" }),
        output: "destructive command",
        is_error: true,
    });

    let output = collect_output_events(&mut events);
    assert!(output.ends_with('\n'), "Output should end with newline: {output:?}");
}

#[test]
fn bash_with_output_transcript_card_has_empty_block_break() {
    let mut theme = crate::ui::theme::Theme::default();
    theme.block_style = crate::ui::theme::BlockStyle::Solid;
    let input = crate::ui::interactive::TranscriptRenderInput {
        item: &crate::ui::interactive::TranscriptItem::tool(crate::ui::interactive::ToolItem {
            name: "bash".to_string(),
            arguments: serde_json::json!({ "command": "cargo test" }),
            is_error: false,
            output: "test result: ok".to_string(),
            output_summary: "test result: ok".to_string(),
            duration_ms: Some(150),
            file_change: None,
        }),
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    };

    let rendered = crate::ui::interactive::render_transcript_item(input);
    assert!(
        rendered.starts_with('\n'),
        "rendered tool card should begin with an empty block break newline"
    );
    let plain = crate::ui::block::strip_ansi(&rendered);
    assert!(plain.contains("cargo test"));
    assert!(plain.contains("test result: ok"));
}

#[test]
fn write_transcript_card_renders_syntax_highlighting_without_backticks() {
    let input = crate::ui::interactive::TranscriptRenderInput {
        item: &crate::ui::interactive::TranscriptItem::tool(crate::ui::interactive::ToolItem {
            name: "write".to_string(),
            arguments: serde_json::json!({
                "path": "test.py",
                "content": "def greet():\n    return 'hello'\n"
            }),
            is_error: false,
            output: "Successfully wrote 33 bytes to test.py".to_string(),
            output_summary: "33 bytes written".to_string(),
            duration_ms: Some(2),
            file_change: None,
        }),
        theme: &crate::ui::theme::Theme::default(),
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    };

    let rendered = crate::ui::interactive::render_transcript_item(input);
    assert!(!rendered.contains("```diff"), "should not have ```diff");
    assert!(!rendered.contains("```"), "should not have ```");
    assert!(!rendered.contains("+ def greet"), "should not have diff + prefix");
    assert!(rendered.contains("greet"), "should contain function name");
}
