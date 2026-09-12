use crate::ui::interactive::transcript::{TranscriptItem, TranscriptRenderInput, render_transcript_item};
use crate::ui::theme::Theme;

#[test]
fn render_transcript_skill_read_collapsed() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(crate::ui::interactive::transcript::ToolItem {
        name: "read".into(),
        arguments: serde_json::json!({"path": "/Users/cadams/.agents/skills/plan/SKILL.md"}),
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
        arguments: serde_json::json!({"path": "/Users/cadams/.agents/skills/plan/SKILL.md"}),
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

fn render_skill_transcript(item: &TranscriptItem, theme: &Theme, expanded: bool) -> String {
    render_transcript_item(TranscriptRenderInput {
        item,
        theme,
        width: 80,
        tools_expanded: expanded,
        hide_thinking: false,
    })
}

#[test]
fn render_transcript_skill_invocation_user_message() {
    let theme = Theme::default();
    let text = "<skill name=\"plan\" location=\"/path/to/SKILL.md\">\nPlan skill body\n</skill>\n\nSkill input: create feature";
    let item = TranscriptItem::UserMessage(text.into());

    let collapsed = render_skill_transcript(&item, &theme, false);
    assert!(
        collapsed.contains("[skill]")
            && collapsed.contains("plan")
            && collapsed.contains("create feature")
            && !collapsed.contains("Plan skill body")
    );

    let expanded = render_skill_transcript(&item, &theme, true);
    assert!(
        expanded.contains("[skill]")
            && expanded.contains("plan")
            && expanded.contains("Plan skill body")
            && expanded.contains("create feature")
    );
}
