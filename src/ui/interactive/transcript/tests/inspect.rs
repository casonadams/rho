use crate::ui::interactive::transcript::{ToolItem, TranscriptItem, TranscriptRenderInput, render_transcript_item};
use crate::ui::theme::Theme;

#[test]
fn render_transcript_standard_read_collapsed_and_expanded() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(ToolItem {
        name: "read".into(),
        arguments: serde_json::json!({"path": "src/main.rs"}),
        is_error: false,
        output: "fn main() { println!(\"hello\"); }".into(),
        output_summary: "summary".into(),
        duration_ms: None,
    });

    let collapsed = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    });
    assert!(collapsed.contains("read"));
    assert!(collapsed.contains("src/main.rs"));
    assert!(!collapsed.contains("println"));

    let expanded = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: true,
        hide_thinking: false,
    });
    assert!(expanded.contains("read"));
    assert!(expanded.contains("src/main.rs"));
    assert!(expanded.contains("println"));
    // Verify syntax highlighting is applied (contains ANSI color escapes)
    assert!(expanded.contains("\x1b["));
}

#[test]
fn render_transcript_web_search_tool_expanded_shows_output() {
    let theme = Theme::default();
    let item = TranscriptItem::Tool(ToolItem {
        name: "web_search".into(),
        arguments: serde_json::json!({"query": "rust async"}),
        is_error: false,
        output: "Found 10 results from crates.io\n1. tokio\n2. futures".into(),
        output_summary: "summary".into(),
        duration_ms: None,
    });

    let collapsed = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: false,
        hide_thinking: false,
    });
    assert!(collapsed.contains("web_search"));
    assert!(collapsed.contains("rust async"));
    assert!(!collapsed.contains("Found 10 results"));

    let expanded = render_transcript_item(TranscriptRenderInput {
        item: &item,
        theme: &theme,
        width: 80,
        tools_expanded: true,
        hide_thinking: false,
    });
    assert!(expanded.contains("web_search"));
    assert!(expanded.contains("Found 10 results from crates.io"));
}
