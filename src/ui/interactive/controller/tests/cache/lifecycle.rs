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

#[test]
fn invariant_items_reuse_standard_across_all_flags() {
    let mut cache = TranscriptRenderCache::new();
    let msg = TranscriptItem::UserMessage("user question".into());
    let theme = Theme::default();

    let first = cache.get_or_render(
        0,
        TranscriptRenderInput {
            item: &msg,
            theme: &theme,
            width: 80,
            tools_expanded: false,
            hide_thinking: false,
        },
    );
    let first_ptr = first.as_ptr();

    let second = cache.get_or_render(
        0,
        TranscriptRenderInput {
            item: &msg,
            theme: &theme,
            width: 80,
            tools_expanded: true,
            hide_thinking: true,
        },
    );
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
    let msg = TranscriptItem::UserMessage("sparse index test".into());
    let theme = Theme::default();

    let rendered = cache.get_or_render(
        3,
        TranscriptRenderInput {
            item: &msg,
            theme: &theme,
            width: 80,
            tools_expanded: false,
            hide_thinking: false,
        },
    );
    assert!(!rendered.is_empty());
    assert_eq!(cache.len(), 4);
    assert!(cache.entry(0).unwrap().standard.is_none());
    assert!(cache.entry(1).unwrap().standard.is_none());
    assert!(cache.entry(2).unwrap().standard.is_none());
    assert!(cache.entry(3).unwrap().standard.is_some());
}
