use super::*;

#[test]
fn render_transcript_welcome() {
    let theme = Theme::default();
    let item = TranscriptItem::Welcome(WelcomeItem {
        version: "0.1.0".into(),
        model: "gpt-4".into(),
        provider: "openai".into(),
        auto_approve: false,
        resumed: false,
        location: ".".into(),
        tools: vec!["read".into(), "write".into(), "playwright_click".into()],
        skills: vec!["plan".into(), "spec".into()],
        plugins: vec!["permission".into()],
    });

    let rendered = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
    });
    assert!(rendered.contains("rho"));
    assert!(rendered.contains("Type /help for commands"));
    assert!(rendered.contains("[skills]"));
    assert!(rendered.contains("plan, spec"));
    assert!(rendered.contains("[tools]"));
    assert!(rendered.contains("read, write"));
    assert!(rendered.contains("[mcp]"));
    assert!(rendered.contains("playwright (1 tool)"));
    assert!(rendered.contains("[plugins]"));
    assert!(rendered.contains("permission"));
}

#[test]
fn render_transcript_user_message() {
    let theme = Theme::default();
    let item = TranscriptItem::UserMessage("hello world".into());
    let rendered = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    assert!(rendered.contains("hello world"));
}

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
    });
    assert!(!rendered.contains("line_one"));
    assert!(rendered.contains("line_ten"));
    assert!(rendered.contains("5 earlier lines, Ctrl+O to expand"));
    assert!(rendered.contains("Took 150ms"));
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
    });
    assert!(rendered.contains("line_one"));
    assert!(rendered.contains("line_ten"));
    assert!(!rendered.contains("earlier lines, Ctrl+O to expand"));
    assert!(rendered.contains("Took 150ms"));
}
