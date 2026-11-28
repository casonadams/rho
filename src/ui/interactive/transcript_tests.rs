use super::*;
use crate::ui::theme::Theme;

fn render_item(item: &TranscriptItem, theme: &Theme, width: usize, tools_expanded: bool) -> String {
    render_transcript_item(TranscriptRenderInput {
        item,
        theme,
        width,
        tools_expanded,
        hide_thinking: false,
    })
}

fn render_tool_item(item: &TranscriptItem, theme: &Theme, expanded: bool) -> String {
    render_item(item, theme, 80, expanded)
}

fn tool_item(
    name: &str,
    arguments: serde_json::Value,
    output: &str,
    output_summary: &str,
    duration_ms: Option<u64>,
) -> TranscriptItem {
    TranscriptItem::tool(ToolItem {
        name: name.into(),
        arguments,
        is_error: false,
        output: output.into(),
        output_summary: output_summary.into(),
        duration_ms,
        file_change: None,
    })
}

#[test]
fn render_transcript_standard_read_collapsed_and_expanded() {
    let theme = Theme::default();
    let item = tool_item(
        "read",
        serde_json::json!({"path": "src/main.rs"}),
        "fn main() { println!(\"hello\"); }",
        "summary",
        None,
    );

    let collapsed = render_tool_item(&item, &theme, false);
    assert!(collapsed.contains("read") && collapsed.contains("src/main.rs") && !collapsed.contains("println"));

    let expanded = render_tool_item(&item, &theme, true);
    assert!(expanded.contains("read") && expanded.contains("src/main.rs") && expanded.contains("println"));
    assert!(expanded.contains("  1 │ "));
    assert!(expanded.contains("\x1b["));
}

#[test]
fn render_transcript_web_search_tool_expanded_shows_output() {
    let theme = Theme::default();
    let item = tool_item(
        "web_search",
        serde_json::json!({"query": "rust async"}),
        "Found 10 results from crates.io\n1. tokio\n2. futures",
        "summary",
        None,
    );

    let collapsed = render_tool_item(&item, &theme, false);
    assert!(
        collapsed.contains("web_search") && collapsed.contains("rust async") && !collapsed.contains("Found 10 results")
    );

    let expanded = render_tool_item(&item, &theme, true);
    assert!(expanded.contains("web_search") && expanded.contains("Found 10 results from crates.io"));
}

#[test]
fn render_transcript_user_message() {
    let theme = Theme::default();
    let item = TranscriptItem::UserMessage("hello world".into());
    let rendered = render_item(&item, &theme, 60, false);
    assert!(rendered.contains("hello world"));
    assert!(rendered.contains('╭'));
    assert!(rendered.contains('╰'));
    assert!(rendered.contains("\x1b[34m"));
}

#[test]
fn render_transcript_user_message_with_border_style() {
    let theme = Theme {
        block_style: BlockStyle::Border,
        user_border: anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlack))),
        ..Default::default()
    };
    let item = TranscriptItem::UserMessage("hello bordered user message".into());
    let rendered = render_item(&item, &theme, 60, false);
    assert!(rendered.contains('╭'));
    assert!(rendered.contains('╰'));
    assert!(rendered.contains('│'));
    assert!(rendered.contains("hello bordered user message"));
    assert!(rendered.contains("\x1b[90m"));
}

#[test]
fn render_transcript_thinking_collapsed_and_expanded() {
    let theme = Theme::default();
    let item = TranscriptItem::Thinking("Let me analyze the code step by step...".into());

    let expanded = render_item(&item, &theme, 80, false);
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
fn render_transcript_thinking_wraps_on_word_boundaries() {
    let theme = Theme::default();
    let item = TranscriptItem::Thinking("alpha beta gamma delta epsilon zeta eta".into());
    let rendered = render_item(&item, &theme, 20, false);
    let lines: Vec<&str> = rendered.trim().lines().collect();
    assert!(lines.len() >= 2);
    assert!(lines[0].contains("alpha beta gamma"));
    assert!(lines[1].contains("delta epsilon"));
}

#[test]
fn render_transcript_assistant_text_emits_osc133_zones() {
    let theme = Theme::default();
    let item = TranscriptItem::AssistantText("Hello from assistant".into());
    let rendered = render_item(&item, &theme, 80, false);
    assert!(rendered.starts_with(OSC133_ZONE_START));
    assert!(rendered.ends_with(&format!("{OSC133_ZONE_END}{OSC133_ZONE_FINAL}")));
    assert!(rendered.contains("Hello from assistant"));
}

#[test]
fn render_transcript_assistant_text_wraps_on_word_boundaries() {
    let theme = Theme::default();
    let text = "This is a long response from the assistant explaining the architecture in detail.";
    let item = TranscriptItem::AssistantText(text.into());
    let rendered = render_item(&item, &theme, 30, false);
    assert!(!rendered.contains("architec-\nture"));
    assert!(rendered.contains("explaining"));
    assert!(rendered.contains("architecture"));
    assert!(rendered.contains("in detail."));
}

#[test]
fn render_transcript_skill_read_collapsed() {
    let theme = Theme::default();
    let item = tool_item(
        "read",
        serde_json::json!({"path": "/Users/cadams/.agents/skills/plan/SKILL.md"}),
        "# Plan Skill\n\nFull instructions here...",
        "summary",
        None,
    );

    let rendered = render_item(&item, &theme, 80, false);
    assert!(rendered.contains("[skill]"));
    assert!(rendered.contains("plan"));
    assert!(!rendered.contains("Full instructions here"));
}

#[test]
fn render_transcript_skill_read_expanded() {
    let theme = Theme::default();
    let item = tool_item(
        "read",
        serde_json::json!({"path": "/Users/cadams/.agents/skills/plan/SKILL.md"}),
        "# Plan Skill\n\nFull instructions here...",
        "summary",
        None,
    );

    let rendered = render_item(&item, &theme, 80, true);
    assert!(rendered.contains("[skill]"));
    assert!(rendered.contains("plan"));
    assert!(rendered.contains("Full instructions here..."));
}

#[test]
fn render_transcript_skill_invocation_user_message() {
    let theme = Theme::default();
    let text = "<skill name=\"plan\" location=\"/path/to/SKILL.md\">\nPlan skill body\n</skill>\n\nSkill input: create feature";
    let item = TranscriptItem::UserMessage(text.into());

    let collapsed = render_tool_item(&item, &theme, false);
    assert!(
        collapsed.contains("[skill]")
            && collapsed.contains("plan")
            && collapsed.contains("create feature")
            && !collapsed.contains("Plan skill body")
    );

    let expanded = render_tool_item(&item, &theme, true);
    assert!(
        expanded.contains("[skill]")
            && expanded.contains("plan")
            && expanded.contains("Plan skill body")
            && expanded.contains("create feature")
    );
}

fn assert_lines_within_width(rendered: &str, max_width: usize) {
    for line in rendered.lines() {
        assert!(crate::ui::block::visible_width(line) <= max_width);
    }
}

const TEN_LINE_OUTPUT: &str =
    "line_one\nline_two\nline_three\nline_four\nline_five\nline_six\nline_seven\nline_eight\nline_nine\nline_ten";

#[test]
fn render_transcript_tool_collapsed_shows_preview() {
    let theme = Theme::default();
    let item = tool_item(
        "bash",
        serde_json::json!({"command": "cargo test"}),
        TEN_LINE_OUTPUT,
        "summary",
        Some(150),
    );

    let rendered = render_tool_item(&item, &theme, false);
    assert!(!rendered.contains("line_one"));
    assert!(rendered.contains("line_ten"));
    assert!(rendered.contains("5 earlier lines"));
    assert!(rendered.contains("Took 150ms"));
}

#[test]
fn render_transcript_tool_output_replaces_tabs_so_block_widths_hold() {
    let theme = Theme::default();
    let item = tool_item(
        "bash",
        serde_json::json!({"command": "cat /etc/hosts"}),
        "##\n127.0.0.1\tlocalhost\n255.255.255.255\tbroadcasthost\n::1\tlocalhost",
        "completed",
        Some(8),
    );

    let rendered = render_tool_item(&item, &theme, false);

    assert!(!rendered.contains('\t'));
    assert_lines_within_width(&rendered, 80);
    assert!(rendered.contains("127.0.0.1") && rendered.contains("localhost"));
}

#[test]
fn render_transcript_tool_expanded_shows_full_output() {
    let theme = Theme::default();
    let item = tool_item(
        "bash",
        serde_json::json!({"command": "cargo test"}),
        TEN_LINE_OUTPUT,
        "summary",
        Some(150),
    );

    let rendered = render_tool_item(&item, &theme, true);
    assert!(rendered.contains("line_one"));
    assert!(rendered.contains("line_ten"));
    assert!(!rendered.contains("earlier lines"));
    assert!(rendered.contains("Took 150ms"));
}

#[test]
fn render_transcript_bash_with_timeout_styles_timeout_with_dimmed() {
    let theme = Theme::default();
    let item = tool_item(
        "bash",
        serde_json::json!({"command": "cargo clippy", "timeout": 300}),
        "finished",
        "completed",
        Some(250),
    );

    let rendered = render_tool_item(&item, &theme, false);

    let dim = theme.dimmed;
    assert!(rendered.contains(&format!("{dim}(timeout 300s){dim:#}")));
    assert!(rendered.contains(&format!("{dim}Took 250ms{dim:#}")));
}

#[test]
fn render_transcript_tool_with_border_style_uses_outline() {
    let theme = Theme {
        block_style: BlockStyle::Border,
        bash_success_border: anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green))),
        ..Default::default()
    };
    let item = tool_item(
        "bash",
        serde_json::json!({"command": "cargo test"}),
        "ok",
        "ok",
        Some(100),
    );

    let rendered = render_item(&item, &theme, 40, false);

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
    let theme = Theme {
        block_style: BlockStyle::Border,
        ..Default::default()
    };
    let item1 = tool_item(
        "rg",
        serde_json::json!({"pattern": "foo"}),
        "match 1",
        "1 match",
        Some(5),
    );
    let item2 = tool_item(
        "read",
        serde_json::json!({"path": "src/main.rs"}),
        "fn main() {}",
        "1 line",
        Some(2),
    );

    let rendered1 = render_item(&item1, &theme, 40, false);
    let rendered2 = render_item(&item2, &theme, 40, false);

    let combined = format!("{rendered1}{rendered2}");
    assert!(
        !combined.contains("╯\x1b[39m\n\n"),
        "bordered tools must touch without an empty line between them"
    );
    let plain = crate::ui::block::strip_ansi(&combined);
    assert!(
        plain.contains("╯\n╭"),
        "bordered tools should transition directly from bottom to top border"
    );
}

#[test]
fn render_transcript_mcp_tool_shows_server_and_call() {
    let theme = Theme::default();
    let item = tool_item(
        "mcp",
        serde_json::json!({
            "action": "call",
            "server": "playwright",
            "tool": "browser_navigate",
            "args": {
                "url": "http://localhost:3000/hub/"
            }
        }),
        "Navigated to http://localhost:3000/hub/",
        "Navigated",
        Some(250),
    );

    let rendered = render_tool_item(&item, &theme, false);

    assert!(rendered.contains("mcp"));
    assert!(rendered.contains("playwright:browser_navigate"));
    assert!(rendered.contains("url=\"http://localhost:3000/hub/\""));
}

fn assert_welcome_content(rendered: &str, expected: &[&str]) {
    for text in expected {
        assert!(rendered.contains(text));
    }
}

fn sample_welcome_item() -> WelcomeItem {
    WelcomeItem {
        version: "0.1.0".into(),
        model: "gpt-4".into(),
        provider: "openai".into(),
        resumed: false,
        location: ".".into(),
        agents: vec!["AGENTS.md".into()],
        tools: vec!["read".into(), "write".into(), "playwright_click".into()],
        skills: vec!["plan".into(), "spec".into()],
        mcp: Vec::new(),
    }
}

#[test]
fn render_transcript_welcome() {
    let theme = Theme::default();
    let item = TranscriptItem::Welcome(sample_welcome_item());

    let rendered = render_tool_item(&item, &theme, false);
    assert_welcome_content(
        &rendered,
        &[
            "rho",
            "Type /help for commands",
            "[agents]",
            "AGENTS.md",
            "[skills]",
            "plan, spec",
            "[tools]",
            "read, write",
            "[mcp]",
            "playwright (1 tool)",
        ],
    );
}

#[test]
fn render_transcript_welcome_without_agents() {
    let theme = Theme::default();
    let item = TranscriptItem::Welcome(WelcomeItem {
        agents: Vec::new(),
        tools: vec!["read".into()],
        skills: Vec::new(),
        ..sample_welcome_item()
    });

    let rendered = render_item(&item, &theme, 80, false);
    assert!(!rendered.contains("[agents]"));
    assert!(!rendered.contains("[skills]"));
    assert!(rendered.contains("[tools]"));
}

#[test]
fn render_transcript_welcome_wraps_long_items_on_word_boundaries() {
    let theme = Theme::default();
    let item = TranscriptItem::Welcome(WelcomeItem {
        version: "0.6.2".into(),
        agents: vec!["~/.agents/AGENTS.md".into(), "AGENTS.md".into()],
        tools: vec!["read".into(), "write".into()],
        skills: vec![
            "dream-weiver".into(),
            "google-agents-cli-adk-code".into(),
            "google-agents-cli-deploy".into(),
            "google-agents-cli-eval".into(),
            "google-agents-cli-observability".into(),
            "google-agents-cli-publish".into(),
            "google-agents-cli-scaffold".into(),
            "google-agents-cli-workflow".into(),
            "plan".into(),
            "spec".into(),
        ],
        mcp: vec!["playwright".into()],
        ..sample_welcome_item()
    });

    let rendered = render_tool_item(&item, &theme, false);
    assert!(!rendered.contains("google-a\n"));
    assert!(!rendered.contains("\n  gents-cli-eval"));
    assert!(rendered.contains("google-agents-cli-eval"));
    assert!(rendered.contains("google-agents-cli-observability"));
    assert!(rendered.contains("google-agents-cli-publish"));
    assert!(rendered.contains("google-agents-cli-scaffold"));
    assert!(rendered.contains("google-agents-cli-workflow"));
}

#[test]
fn render_transcript_welcome_with_mcp_servers() {
    let theme = Theme::default();
    let item = TranscriptItem::Welcome(WelcomeItem {
        version: "0.7.1".into(),
        agents: Vec::new(),
        tools: vec![
            "read".into(),
            "write".into(),
            "mcp".into(),
            "mcpScript".into(),
            "tool_search".into(),
        ],
        skills: Vec::new(),
        mcp: vec!["playwright".into()],
        ..sample_welcome_item()
    });

    let rendered = render_tool_item(&item, &theme, false);
    assert_welcome_content(
        &rendered,
        &["[tools]", "read, write, tool_search", "[mcp]", "playwright"],
    );
    assert!(!rendered.contains("tool (1 tool)"), "{rendered}");
    assert!(!rendered.contains("mcpScript"));
}

#[test]
fn render_transcript_welcome_lists_every_builtin_tool_and_no_custom_section() {
    let theme = Theme::default();
    let names = ["read", "undo", "lsp", "todo", "memory", "subagent", "worktree"];
    let item = TranscriptItem::Welcome(WelcomeItem {
        version: "0.7.1".into(),
        agents: Vec::new(),
        tools: names.iter().map(ToString::to_string).collect(),
        skills: Vec::new(),
        mcp: Vec::new(),
        ..sample_welcome_item()
    });

    let rendered = render_tool_item(&item, &theme, false);
    assert_welcome_content(
        &rendered,
        &["[tools]", "read, undo, lsp, todo, memory, subagent, worktree"],
    );
    assert!(!rendered.contains("[custom]"), "{rendered}");
}
