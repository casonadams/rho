use crate::ui::interactive::transcript::{
    OSC133_ZONE_END, OSC133_ZONE_FINAL, OSC133_ZONE_START, TranscriptItem, TranscriptRenderInput,
    render_transcript_item,
};
use crate::ui::theme::Theme;

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
