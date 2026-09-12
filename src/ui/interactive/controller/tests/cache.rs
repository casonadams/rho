//! Transcript render cache tests: slot classification, lazy population, and
//! cache lifecycle behavior.

mod classification {
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
            mcp: Vec::new(),
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
}

mod lazy {
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
        item: &TranscriptItem,
        theme: &Theme,
        expanded: bool,
        hide: bool,
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

        let collapsed = render_cached(&mut cache, &tool, &theme, false, false);
        let entry = cache.entry(0).unwrap();
        assert!(entry.standard.is_some() && entry.alternate.is_none());

        let expanded = render_cached(&mut cache, &tool, &theme, true, false);
        let entry = cache.entry(0).unwrap();
        assert!(entry.standard.is_some() && entry.alternate.is_some() && collapsed != expanded);

        let collapsed_second = render_cached(&mut cache, &tool, &theme, false, false);
        assert_eq!(collapsed_second, collapsed);

        let expanded_second = render_cached(&mut cache, &tool, &theme, true, false);
        assert_eq!(expanded_second, expanded);
    }

    #[test]
    fn thinking_caching_populates_both_standard_and_alternate_lazily() {
        let mut cache = TranscriptRenderCache::new();
        let (thinking, theme) = (TranscriptItem::Thinking("internal thoughts".into()), Theme::default());

        let visible = render_cached(&mut cache, &thinking, &theme, false, false);
        let hidden = render_cached(&mut cache, &thinking, &theme, false, true);

        assert_ne!(visible, hidden);
        assert!(hidden.contains("Thinking..."));
        assert_eq!(cache.get(0, RenderSlot::Standard), Some(visible.as_str()));
        assert_eq!(cache.get(0, RenderSlot::Alternate), Some(hidden.as_str()));
    }
}

mod lifecycle {
    use crate::ui::interactive::controller::cache::{CachedItemRender, TranscriptRenderCache};
    use crate::ui::interactive::{TranscriptItem, TranscriptRenderInput};
    use crate::ui::theme::Theme;

    #[test]
    fn cache_hit_returns_existing_reference_without_re_rendering() {
        let mut cache = TranscriptRenderCache::new();
        let user_msg = TranscriptItem::UserMessage("hello".into());
        let theme = Theme::default();

        cache.set(0, CachedItemRender::standard("SENTINEL_OUTPUT"));

        let rendered = cache.get_or_render(
            0,
            TranscriptRenderInput {
                item: &user_msg,
                theme: &theme,
                width: 80,
                tools_expanded: false,
                hide_thinking: false,
            },
        );
        assert_eq!(rendered, "SENTINEL_OUTPUT");
    }

    fn render_input<'a>(
        item: &'a TranscriptItem,
        theme: &'a Theme,
        expanded: bool,
        hide: bool,
    ) -> TranscriptRenderInput<'a> {
        TranscriptRenderInput {
            item,
            theme,
            width: 80,
            tools_expanded: expanded,
            hide_thinking: hide,
        }
    }

    #[test]
    fn invariant_items_reuse_standard_across_all_flags() {
        let mut cache = TranscriptRenderCache::new();
        let (msg, theme) = (TranscriptItem::UserMessage("user question".into()), Theme::default());

        let first = cache.get_or_render(0, render_input(&msg, &theme, false, false));
        let first_ptr = first.as_ptr();

        let second = cache.get_or_render(0, render_input(&msg, &theme, true, true));
        assert_eq!(second.as_ptr(), first_ptr);
        assert!(cache.entry(0).unwrap().alternate.is_none());
    }

    #[test]
    fn cache_clear_invalidates_all_entries() {
        let mut cache = TranscriptRenderCache::new();
        let msg = TranscriptItem::UserMessage("testing clear".into());
        let theme = Theme::default();

        cache.get_or_render(
            0,
            TranscriptRenderInput {
                item: &msg,
                theme: &theme,
                width: 80,
                tools_expanded: false,
                hide_thinking: false,
            },
        );
        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn get_or_render_resizes_cache_when_index_is_out_of_bounds() {
        let mut cache = TranscriptRenderCache::new();
        let (msg, theme) = (
            TranscriptItem::UserMessage("sparse index test".into()),
            Theme::default(),
        );

        let rendered = cache.get_or_render(3, render_input(&msg, &theme, false, false));
        assert!(!rendered.is_empty() && cache.len() == 4);
        for idx in 0..3 {
            assert!(cache.entry(idx).unwrap().standard.is_none());
        }
        assert!(cache.entry(3).unwrap().standard.is_some());
    }
}
