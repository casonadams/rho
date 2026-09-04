use crate::ui::interactive::transcript::{TranscriptItem, TranscriptRenderInput, render_transcript_item};
use crate::ui::theme::Theme;

#[test]
fn render_transcript_skill_read_collapsed() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(crate::ui::interactive::transcript::ToolItem {
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
    assert!(!rendered.contains("Full instructions here"));
}

#[test]
fn render_transcript_skill_read_expanded() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(crate::ui::interactive::transcript::ToolItem {
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
