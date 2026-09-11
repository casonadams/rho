use crate::ui::interactive::transcript::{ToolItem, TranscriptItem, TranscriptRenderInput, render_transcript_item};
use crate::ui::theme::Theme;

#[test]
fn render_transcript_tool_collapsed_shows_preview() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(ToolItem {
        name: "bash".into(),
        arguments: serde_json::json!({"command": "cargo test"}),
        is_error: false,
        output: "line_one\nline_two\nline_three\nline_four\nline_five\nline_six\nline_seven\nline_eight\nline_nine\nline_ten".into(),
        output_summary: "summary".into(),
        duration_ms: Some(150),
    });

    let rendered = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    });
    assert!(!rendered.contains("line_one"));
    assert!(rendered.contains("line_ten"));
    assert!(rendered.contains("5 earlier lines"));
    assert!(rendered.contains("Took 150ms"));
}

fn assert_lines_within_width(rendered: &str, max_width: usize) {
    for line in rendered.lines() {
        assert!(crate::ui::block::visible_width(line) <= max_width);
    }
}

#[test]
fn render_transcript_tool_output_replaces_tabs_so_block_widths_hold() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(ToolItem {
        name: "bash".into(),
        arguments: serde_json::json!({"command": "cat /etc/hosts"}),
        is_error: false,
        output: "##\n127.0.0.1\tlocalhost\n255.255.255.255\tbroadcasthost\n::1\tlocalhost".into(),
        output_summary: "completed".into(),
        duration_ms: Some(8),
    });

    let rendered = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    });

    assert!(!rendered.contains('\t'));
    assert_lines_within_width(&rendered, 80);
    assert!(rendered.contains("127.0.0.1") && rendered.contains("localhost"));
}

#[test]
fn render_transcript_tool_expanded_shows_full_output() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(ToolItem {
        name: "bash".into(),
        arguments: serde_json::json!({"command": "cargo test"}),
        is_error: false,
        output: "line_one\nline_two\nline_three\nline_four\nline_five\nline_six\nline_seven\nline_eight\nline_nine\nline_ten".into(),
        output_summary: "summary".into(),
        duration_ms: Some(150),
    });

    let rendered = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: true,
        hide_thinking: false,
    });
    assert!(rendered.contains("line_one"));
    assert!(rendered.contains("line_ten"));
    assert!(!rendered.contains("earlier lines"));
    assert!(rendered.contains("Took 150ms"));
}

#[test]
fn render_transcript_bash_with_timeout_styles_timeout_with_dimmed() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(ToolItem {
        name: "bash".into(),
        arguments: serde_json::json!({"command": "cargo clippy", "timeout": 300}),
        is_error: false,
        output: "finished".into(),
        output_summary: "completed".into(),
        duration_ms: Some(250),
    });

    let rendered = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    });

    let dim = theme.dimmed;
    assert!(rendered.contains(&format!("{dim}(timeout 300s){dim:#}")));
    assert!(rendered.contains(&format!("{dim}Took 250ms{dim:#}")));
}
