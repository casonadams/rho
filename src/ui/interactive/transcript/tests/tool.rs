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
    assert!(rendered.contains("5 earlier lines, Ctrl+O to expand"));
    assert!(rendered.contains("Took 150ms"));
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

    assert!(!rendered.contains('\t'), "tabs must not reach the terminal");
    for line in rendered.lines() {
        let visible = crate::ui::block::visible_width(line);
        assert!(visible <= 80, "line renders {visible} cols, wider than block");
    }
    assert!(rendered.contains("127.0.0.1"));
    assert!(rendered.contains("localhost"));
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
    assert!(!rendered.contains("earlier lines, Ctrl+O to expand"));
    assert!(rendered.contains("Took 150ms"));
}

#[test]
fn render_transcript_standard_read_collapsed_and_expanded() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(ToolItem {
        name: "read".into(),
        arguments: serde_json::json!({"path": "src/main.rs"}),
        is_error: false,
        output: "fn main() { println!(\"hello\"); }".into(),
        output_summary: "summary".into(),
        duration_ms: None,
    });

    let collapsed = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    });
    assert!(collapsed.contains("read"));
    assert!(collapsed.contains("src/main.rs"));
    assert!(collapsed.contains("(ctrl+o to expand)"));
    assert!(!collapsed.contains("println"));

    let expanded = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: true,
        hide_thinking: false,
    });
    assert!(expanded.contains("read"));
    assert!(expanded.contains("src/main.rs"));
    assert!(!expanded.contains("(ctrl+o to expand)"));
    assert!(expanded.contains("println"));
    // Verify syntax highlighting is applied (contains ANSI color escapes)
    assert!(expanded.contains("\x1b["));
}

#[test]
fn render_transcript_web_search_tool_expanded_shows_output() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(ToolItem {
        name: "web_search".into(),
        arguments: serde_json::json!({"query": "rust async"}),
        is_error: false,
        output: "Found 10 results from crates.io\n1. tokio\n2. futures".into(),
        output_summary: "summary".into(),
        duration_ms: None,
    });

    let collapsed = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    });
    assert!(collapsed.contains("web_search"));
    assert!(collapsed.contains("rust async"));
    assert!(!collapsed.contains("Found 10 results"));

    let expanded = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: true,
        hide_thinking: false,
    });
    assert!(expanded.contains("web_search"));
    assert!(expanded.contains("Found 10 results from crates.io"));
}
