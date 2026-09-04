use crate::ui::interactive::controller::cache::{RenderSlot, TranscriptRenderCache};
use crate::ui::interactive::{ToolItem, TranscriptItem, TranscriptRenderInput};
use crate::ui::theme::Theme;

fn sample_tool() -> TranscriptItem {
    TranscriptItem::Tool(ToolItem {
        name: "read".into(),
        arguments: serde_json::json!({"path": "src/main.rs"}),
        is_error: false,
        output: "fn main() {\n    println!(\"hello\");\n}".into(),
        output_summary: "3 lines".into(),
        duration_ms: None,
    })
}

#[test]
fn tool_caching_populates_both_standard_and_alternate_lazily() {
    let mut cache = TranscriptRenderCache::new();
    let tool = sample_tool();
    let theme = Theme::default();

    let collapsed = cache
        .get_or_render(
            0,
            TranscriptRenderInput {
                item: &tool,
                theme: &theme,
                width: 80,
                tools_expanded: false,
                hide_thinking: false,
            },
        )
        .to_string();

    assert!(cache.entry(0).unwrap().standard.is_some());
    assert!(cache.entry(0).unwrap().alternate.is_none());

    let expanded = cache
        .get_or_render(
            0,
            TranscriptRenderInput {
                item: &tool,
                theme: &theme,
                width: 80,
                tools_expanded: true,
                hide_thinking: false,
            },
        )
        .to_string();

    assert!(cache.entry(0).unwrap().standard.is_some());
    assert!(cache.entry(0).unwrap().alternate.is_some());
    assert_ne!(collapsed, expanded);

    let collapsed_second = cache.get_or_render(
        0,
        TranscriptRenderInput {
            item: &tool,
            theme: &theme,
            width: 80,
            tools_expanded: false,
            hide_thinking: false,
        },
    );
    assert_eq!(collapsed_second, collapsed);

    let expanded_second = cache.get_or_render(
        0,
        TranscriptRenderInput {
            item: &tool,
            theme: &theme,
            width: 80,
            tools_expanded: true,
            hide_thinking: false,
        },
    );
    assert_eq!(expanded_second, expanded);
}

#[test]
fn thinking_caching_populates_both_standard_and_alternate_lazily() {
    let mut cache = TranscriptRenderCache::new();
    let thinking = TranscriptItem::Thinking("internal thoughts".into());
    let theme = Theme::default();

    let visible = cache
        .get_or_render(
            0,
            TranscriptRenderInput {
                item: &thinking,
                theme: &theme,
                width: 80,
                tools_expanded: false,
                hide_thinking: false,
            },
        )
        .to_string();

    let hidden = cache
        .get_or_render(
            0,
            TranscriptRenderInput {
                item: &thinking,
                theme: &theme,
                width: 80,
                tools_expanded: false,
                hide_thinking: true,
            },
        )
        .to_string();

    assert_ne!(visible, hidden);
    assert!(hidden.contains("Thinking..."));
    assert_eq!(cache.get(0, RenderSlot::Standard), Some(visible.as_str()));
    assert_eq!(cache.get(0, RenderSlot::Alternate), Some(hidden.as_str()));
}
