use crate::ui::interactive::controller::cache::{
    CachedItemRender, RenderSlot, TranscriptRenderCache, is_dual_state, is_invariant, target_slot,
};
use crate::ui::interactive::{ToolItem, TranscriptItem, WelcomeItem};

fn sample_welcome() -> TranscriptItem {
    TranscriptItem::Welcome(WelcomeItem {
        version: "1.0.0".into(),
        model: "gpt-4o".into(),
        provider: "openai".into(),
        resumed: false,
        location: "/tmp".into(),
        agents: Vec::new(),
        tools: Vec::new(),
        skills: Vec::new(),
        plugins: Vec::new(),
    })
}

fn sample_tool() -> TranscriptItem {
    TranscriptItem::Tool(ToolItem {
        name: "bash".into(),
        arguments: serde_json::json!({"command": "ls -la"}),
        is_error: false,
        output: "file1.txt\nfile2.txt".into(),
        output_summary: "2 files".into(),
        duration_ms: Some(15),
    })
}

#[test]
fn invariant_items_use_standard_slot_and_are_classified_correctly() {
    let welcome = sample_welcome();
    let user_msg = TranscriptItem::UserMessage("hello world".into());
    let assistant = TranscriptItem::AssistantText("assistant reply".into());
    let notice = TranscriptItem::Notice("system notice".into());

    for item in [&welcome, &user_msg, &assistant, &notice] {
        assert!(is_invariant(item));
        assert!(!is_dual_state(item));
        assert_eq!(target_slot(item, false, false), RenderSlot::Standard);
        assert_eq!(target_slot(item, true, false), RenderSlot::Standard);
        assert_eq!(target_slot(item, false, true), RenderSlot::Standard);
        assert_eq!(target_slot(item, true, true), RenderSlot::Standard);
    }
}

#[test]
fn dual_state_items_route_to_standard_or_alternate_slot() {
    let tool = sample_tool();
    let thinking = TranscriptItem::Thinking("pondering deeply...".into());

    assert!(!is_invariant(&tool));
    assert!(is_dual_state(&tool));
    assert_eq!(target_slot(&tool, false, false), RenderSlot::Standard);
    assert_eq!(target_slot(&tool, true, false), RenderSlot::Alternate);

    assert!(!is_invariant(&thinking));
    assert!(is_dual_state(&thinking));
    assert_eq!(target_slot(&thinking, false, false), RenderSlot::Standard);
    assert_eq!(target_slot(&thinking, false, true), RenderSlot::Alternate);
}

#[test]
fn cached_item_render_get_and_set() {
    let mut item = CachedItemRender::default();
    assert_eq!(item.get(RenderSlot::Standard), None);
    assert_eq!(item.get(RenderSlot::Alternate), None);

    item.set(RenderSlot::Standard, "rendered standard");
    assert_eq!(item.get(RenderSlot::Standard), Some("rendered standard"));
    assert_eq!(item.get(RenderSlot::Alternate), None);

    item.set(RenderSlot::Alternate, "rendered alternate");
    assert_eq!(item.get(RenderSlot::Standard), Some("rendered standard"));
    assert_eq!(item.get(RenderSlot::Alternate), Some("rendered alternate"));
}

#[test]
fn cache_push_records_to_target_slot() {
    let mut cache = TranscriptRenderCache::new();
    let tool = sample_tool();

    let collapsed_slot = target_slot(&tool, false, false);
    cache.push(collapsed_slot, "collapsed tool");
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.get(0, RenderSlot::Standard), Some("collapsed tool"));
    assert_eq!(cache.get(0, RenderSlot::Alternate), None);

    let expanded_slot = target_slot(&tool, true, false);
    cache.push(expanded_slot, "expanded tool");
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get(1, RenderSlot::Standard), None);
    assert_eq!(cache.get(1, RenderSlot::Alternate), Some("expanded tool"));
}
