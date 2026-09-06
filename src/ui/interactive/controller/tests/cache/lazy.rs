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

fn render_cached(
    cache: &mut TranscriptRenderCache,
    (item, theme): (&TranscriptItem, &Theme),
    (expanded, hide): (bool, bool),
) -> String {
    cache
        .get_or_render(
            0,
            TranscriptRenderInput {
                item,
                theme,
                width: 80,
                tools_expanded: expanded,
                hide_thinking: hide,
            },
        )
        .to_string()
}

#[test]
fn tool_caching_populates_both_standard_and_alternate_lazily() {
    let mut cache = TranscriptRenderCache::new();
    let (tool, theme) = (sample_tool(), Theme::default());

    let collapsed = render_cached(&mut cache, (&tool, &theme), (false, false));
    let entry = cache.entry(0).unwrap();
    assert!(entry.standard.is_some() && entry.alternate.is_none());

    let expanded = render_cached(&mut cache, (&tool, &theme), (true, false));
    let entry = cache.entry(0).unwrap();
    assert!(entry.standard.is_some() && entry.alternate.is_some() && collapsed != expanded);

    let collapsed_second = render_cached(&mut cache, (&tool, &theme), (false, false));
    assert_eq!(collapsed_second, collapsed);

    let expanded_second = render_cached(&mut cache, (&tool, &theme), (true, false));
    assert_eq!(expanded_second, expanded);
}

#[test]
fn thinking_caching_populates_both_standard_and_alternate_lazily() {
    let mut cache = TranscriptRenderCache::new();
    let (thinking, theme) = (TranscriptItem::Thinking("internal thoughts".into()), Theme::default());

    let visible = render_cached(&mut cache, (&thinking, &theme), (false, false));
    let hidden = render_cached(&mut cache, (&thinking, &theme), (false, true));

    assert_ne!(visible, hidden);
    assert!(hidden.contains("Thinking..."));
    assert_eq!(cache.get(0, RenderSlot::Standard), Some(visible.as_str()));
    assert_eq!(cache.get(0, RenderSlot::Alternate), Some(hidden.as_str()));
}
