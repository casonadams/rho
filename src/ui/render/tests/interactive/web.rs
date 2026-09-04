use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InteractiveUi, OutputEvent, UiEvent};
use rho_harness_core::presentation::ToolLine;

#[test]
fn fetch_renders_url_on_same_line_without_duplicate() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.finish_tool_line(ToolLine {
        name: "web_fetch".to_string(),
        arguments: serde_json::json!({"url": "https://serde.rs/"}),
        is_error: false,
        output: "serde docs".to_string(),
        output_summary: "serde docs".to_string(),
        duration_ms: None,
    });

    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Transcript(item) => output.push_str(&crate::ui::interactive::render_transcript_item(
                crate::ui::interactive::TranscriptRenderInput {
                    item: &item,
                    theme: &renderer.theme,
                    width: 80,
                    tools_expanded: false,
                    hide_thinking: false,
                },
            )),
            UiEvent::Output(OutputEvent::Text(text)) => output.push_str(&text),
            _ => {}
        }
    }
    assert!(output.contains("web_fetch"));
    assert!(output.contains("https://serde.rs/"));
    assert!(output.contains("fetched (text)"));
    assert_eq!(output.matches("https://serde.rs/").count(), 1);
}

#[test]
fn search_tool_displays_cleanly() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.finish_tool_line(ToolLine {
        name: "web_search".to_string(),
        arguments: serde_json::json!({"query": "serde release"}),
        is_error: false,
        output: "results".to_string(),
        output_summary: "results".to_string(),
        duration_ms: None,
    });

    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Transcript(item) => output.push_str(&crate::ui::interactive::render_transcript_item(
                crate::ui::interactive::TranscriptRenderInput {
                    item: &item,
                    theme: &renderer.theme,
                    width: 80,
                    tools_expanded: false,
                    hide_thinking: false,
                },
            )),
            UiEvent::Output(OutputEvent::Text(text)) => output.push_str(&text),
            _ => {}
        }
    }
    assert!(output.contains("web_search"));
    assert!(output.contains("\"serde release\""));
}
