use crate::ui::interactive::transcript::{ToolItem, TranscriptItem, TranscriptRenderInput, render_transcript_item};
use crate::ui::theme::Theme;

fn render_tool_item(item: &TranscriptItem, theme: &Theme, expanded: bool) -> String {
    render_transcript_item(TranscriptRenderInput {
        item,
        theme,
        width: 80,
        tools_expanded: expanded,
        hide_thinking: false,
    })
}

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

    let collapsed = render_tool_item(&item, &theme, false);
    assert!(collapsed.contains("read") && collapsed.contains("src/main.rs") && !collapsed.contains("println"));

    let expanded = render_tool_item(&item, &theme, true);
    assert!(expanded.contains("read") && expanded.contains("src/main.rs") && expanded.contains("println"));
    assert!(expanded.contains("  1 │ "));
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

    let collapsed = render_tool_item(&item, &theme, false);
    assert!(
        collapsed.contains("web_search") && collapsed.contains("rust async") && !collapsed.contains("Found 10 results")
    );

    let expanded = render_tool_item(&item, &theme, true);
    assert!(expanded.contains("web_search") && expanded.contains("Found 10 results from crates.io"));
}
