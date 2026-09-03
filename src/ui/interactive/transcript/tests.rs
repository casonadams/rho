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
        hide_thinking: false,
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
        hide_thinking: false,
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
fn render_transcript_skill_read_collapsed() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(ToolItem {
        name: "read".into(),
        arguments: serde_json::json!({"path": "/Users/cadams/.pi/agent/skills/plan/SKILL.md"}),
        is_error: false,
        output: "# Plan Skill\n\nFull instructions here...".into(),
        output_summary: "summary".into(),
        duration_ms: None,
    });

    let rendered = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    });
    assert!(rendered.contains("[skill]"));
    assert!(rendered.contains("plan"));
    assert!(rendered.contains("(ctrl+o to expand)"));
    assert!(!rendered.contains("Full instructions here"));
}

#[test]
fn render_transcript_skill_read_expanded() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(ToolItem {
        name: "read".into(),
        arguments: serde_json::json!({"path": "/Users/cadams/.pi/agent/skills/plan/SKILL.md"}),
        is_error: false,
        output: "# Plan Skill\n\nFull instructions here...".into(),
        output_summary: "summary".into(),
        duration_ms: None,
    });

    let rendered = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: true,
        hide_thinking: false,
    });
    assert!(rendered.contains("[skill]"));
    assert!(rendered.contains("plan"));
    assert!(rendered.contains("Full instructions here..."));
}

#[test]
fn render_transcript_skill_invocation_user_message() {
    let theme = Theme::default();
    let text = "<skill name=\"plan\" location=\"/path/to/SKILL.md\">\nPlan skill body\n</skill>\n\nSkill input: create feature";
    let item = TranscriptItem::UserMessage(text.into());

    let collapsed = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    });
    assert!(collapsed.contains("[skill]"));
    assert!(collapsed.contains("plan"));
    assert!(collapsed.contains("(ctrl+o to expand)"));
    assert!(collapsed.contains("create feature"));
    assert!(!collapsed.contains("Plan skill body"));

    let expanded = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: true,
        hide_thinking: false,
    });
    assert!(expanded.contains("[skill]"));
    assert!(expanded.contains("plan"));
    assert!(expanded.contains("Plan skill body"));
    assert!(expanded.contains("create feature"));
}

#[test]
fn render_transcript_thinking_collapsed_and_expanded() {
    let theme = Theme::default();
    let item = TranscriptItem::Thinking("Let me analyze the code step by step...".into());

    let expanded = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    });
    assert!(expanded.contains("analyze the code"));

    let collapsed = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: true,
    });
    assert!(collapsed.contains("Thinking..."));
    assert!(!collapsed.contains("analyze the code"));
}

#[test]
fn render_transcript_assistant_text_emits_osc133_zones() {
    let theme = Theme::default();
    let item = TranscriptItem::AssistantText("Hello from assistant".into());
    let rendered = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    });
    assert!(rendered.starts_with(OSC133_ZONE_START));
    assert!(rendered.ends_with(&format!("{OSC133_ZONE_END}{OSC133_ZONE_FINAL}")));
    assert!(rendered.contains("Hello from assistant"));
}

#[test]
fn render_transcript_search_tool_expanded_shows_output() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(ToolItem {
        name: "search".into(),
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
    assert!(collapsed.contains("search"));
    assert!(collapsed.contains("rust async"));
    assert!(!collapsed.contains("Found 10 results"));

    let expanded = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: true,
        hide_thinking: false,
    });
    assert!(expanded.contains("search"));
    assert!(expanded.contains("Found 10 results from crates.io"));
}
