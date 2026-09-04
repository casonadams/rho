use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InteractiveUi, UiEvent};
use crate::ui::render::CacheMissNotice;
use crate::ui::render::formatters::format_session_status;
use crate::ui::render::preview::{fetch_content_kind, tool_title_style};
use rho_harness_core::presentation::SessionStatus;

#[test]
fn print_session_status_and_notice_emit_transcript_item() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.print_session_status(&SessionStatus {
        model: "claude-sonnet".to_string(),
        provider: "anthropic".to_string(),
        context: "42% context".to_string(),
        quota: Some("80% quota".to_string()),
        auto_approve: true,
    });
    renderer.print_notice("  [Notice message]\n");

    let items = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Notice(text)) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(items.len(), 2);
    assert!(items[0].contains("claude-sonnet"));
    assert!(items[0].contains("42% context"));
    assert!(items[1].contains("[Notice message]"));
}

#[test]
fn print_compaction_and_cache_miss_notices() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.print_compaction_cost_notice(154_000, Some(0.46));
    renderer.print_cache_miss_notice(CacheMissNotice {
        missed_tokens: 45_000,
        cost: Some(0.14),
        idle_minutes: Some(5),
    });

    let items = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Notice(text)) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(items.len(), 2);
    assert!(items[0].contains("Compaction: 154k tokens billed (~$0.46)"));
    assert!(items[1].contains("Cache miss after 5m idle: 45k tokens re-billed (~$0.14)"));
}

#[test]
fn error_tool_titles_use_terminal_red_without_dimming() {
    assert_eq!(tool_title_style(false).render().to_string(), "\x1b[1m");
    assert_eq!(tool_title_style(true).render().to_string(), "\x1b[1m\x1b[31m");
}

#[test]
fn fetch_content_kind_uses_format_or_url_extension() {
    assert_eq!(
        fetch_content_kind(&serde_json::json!({"url": "https://example.com/page"})),
        "text"
    );
    assert_eq!(
        fetch_content_kind(&serde_json::json!({"url": "https://example.com/data.json"})),
        "json"
    );
    assert_eq!(
        fetch_content_kind(&serde_json::json!({"url": "https://example.com/file", "format": "pdf"})),
        "pdf"
    );
}

#[test]
fn session_status_keeps_runtime_context_visible() {
    assert_eq!(
        format_session_status(&SessionStatus {
            model: "claude-sonnet".to_string(),
            provider: "anthropic".to_string(),
            context: "27.4% (1M)".to_string(),
            quota: Some("93% (3h22m)".to_string()),
            auto_approve: false,
        }),
        "claude-sonnet | 27.4% (1M) | 93% (3h22m)"
    );
    assert_eq!(
        format_session_status(&SessionStatus {
            model: "qwen".to_string(),
            provider: "ollama".to_string(),
            context: "0% (376k)".to_string(),
            quota: None,
            auto_approve: true,
        }),
        "qwen | 0% (376k)"
    );
}
