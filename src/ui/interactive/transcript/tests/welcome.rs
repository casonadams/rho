use crate::ui::interactive::transcript::{TranscriptItem, TranscriptRenderInput, WelcomeItem, render_transcript_item};
use crate::ui::theme::Theme;

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
