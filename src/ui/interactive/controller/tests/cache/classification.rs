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

fn assert_invariant_classification(item: &TranscriptItem) {
    assert!(is_invariant(item) && !is_dual_state(item));
    for (exp, hide) in [(false, false), (true, false), (false, true), (true, true)] {
        assert_eq!(target_slot(item, exp, hide), RenderSlot::Standard);
    }
}

#[test]
fn invariant_items_use_standard_slot_and_are_classified_correctly() {
    let welcome = sample_welcome();
    let user_msg = TranscriptItem::UserMessage("hello world".into());
    let assistant = TranscriptItem::AssistantText("assistant reply".into());
    let notice = TranscriptItem::Notice("system notice".into());

    for item in [&welcome, &user_msg, &assistant, &notice] {
        assert_invariant_classification(item);
    }
}

#[test]
fn dual_state_tool_slot_routing() {
    let tool = sample_tool();
    assert!(!is_invariant(&tool) && is_dual_state(&tool));
    assert_eq!(target_slot(&tool, false, false), RenderSlot::Standard);
    assert_eq!(target_slot(&tool, true, false), RenderSlot::Alternate);
}

#[test]
fn dual_state_thinking_slot_routing() {
    let thinking = TranscriptItem::Thinking("pondering deeply...".into());
    assert!(!is_invariant(&thinking) && is_dual_state(&thinking));
    assert_eq!(target_slot(&thinking, false, false), RenderSlot::Standard);
    assert_eq!(target_slot(&thinking, false, true), RenderSlot::Alternate);
}

#[test]
fn cached_item_render_initial_and_standard() {
    let mut item = CachedItemRender::default();
    assert_eq!(
        (item.get(RenderSlot::Standard), item.get(RenderSlot::Alternate)),
        (None, None)
    );

    item.set(RenderSlot::Standard, "rendered standard");
    assert_eq!(item.get(RenderSlot::Standard), Some("rendered standard"));
    assert_eq!(item.get(RenderSlot::Alternate), None);
}

#[test]
fn cached_item_render_alternate() {
    let mut item = CachedItemRender::default();
    item.set(RenderSlot::Standard, "rendered standard");
    item.set(RenderSlot::Alternate, "rendered alternate");
    assert_eq!(item.get(RenderSlot::Standard), Some("rendered standard"));
    assert_eq!(item.get(RenderSlot::Alternate), Some("rendered alternate"));
}

#[test]
fn cache_push_records_to_target_slot() {
    let mut cache = TranscriptRenderCache::new();
    let tool = sample_tool();

    cache.push(target_slot(&tool, false, false), "collapsed tool");
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.get(0, RenderSlot::Standard), Some("collapsed tool"));

    cache.push(target_slot(&tool, true, false), "expanded tool");
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get(1, RenderSlot::Alternate), Some("expanded tool"));
}
