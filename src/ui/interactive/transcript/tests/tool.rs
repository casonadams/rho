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

#[test]
fn render_transcript_tool_with_border_style_uses_outline() {
    let mut theme = Theme::default();
    theme.block_style = crate::ui::theme::BlockStyle::Border;
    theme.bash_success_border = anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green)));
    let item = TranscriptItem::Tool(ToolItem {
        name: "bash".into(),
        arguments: serde_json::json!({"command": "cargo test"}),
        is_error: false,
        output: "ok".into(),
        output_summary: "ok".into(),
        duration_ms: Some(100),
    });

    let rendered = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 40,
        tools_expanded: false,
        hide_thinking: false,
    });

    assert!(rendered.contains('╭'));
    assert!(rendered.contains('╰'));
    assert!(rendered.contains('│'));
    assert!(rendered.contains("\x1b[32m"));
    assert!(
        !rendered.starts_with('\n'),
        "border mode tool items should not have leading newline"
    );
}

#[test]
fn consecutive_bordered_tool_items_have_no_empty_lines_between_them() {
    let mut theme = Theme::default();
    theme.block_style = crate::ui::theme::BlockStyle::Border;
    let item1 = TranscriptItem::Tool(ToolItem {
        name: "rg".into(),
        arguments: serde_json::json!({"pattern": "foo"}),
        is_error: false,
        output: "match 1".into(),
        output_summary: "1 match".into(),
        duration_ms: Some(5),
    });
    let item2 = TranscriptItem::Tool(ToolItem {
        name: "read".into(),
        arguments: serde_json::json!({"path": "src/main.rs"}),
        is_error: false,
        output: "fn main() {}".into(),
        output_summary: "1 line".into(),
        duration_ms: Some(2),
    });

    let rendered1 = render_transcript_item(TranscriptRenderInput {
        item: &item1,
        theme: &theme,
        width: 40,
        tools_expanded: false,
        hide_thinking: false,
    });
    let rendered2 = render_transcript_item(TranscriptRenderInput {
        item: &item2,
        theme: &theme,
        width: 40,
        tools_expanded: false,
        hide_thinking: false,
    });

    let combined = format!("{rendered1}{rendered2}");
    assert!(
        !combined.contains("╯\x1b[39m\n\n"),
        "bordered tools must touch without an empty line between them"
    );
    let plain = crate::ui::block::ANSI_PATTERN.replace_all(&combined, "");
    assert!(
        plain.contains("╯\n╭"),
        "bordered tools should transition directly from bottom to top border"
    );
}
