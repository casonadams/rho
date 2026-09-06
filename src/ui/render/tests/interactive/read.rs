use crate::ui::TerminalRenderer;
use crate::ui::interactive::{InteractiveUi, OutputEvent, UiEvent};
use rho_harness_core::presentation::ToolLine;

fn collect_rendered_transcript(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    theme: &crate::ui::theme::Theme,
) -> String {
    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Transcript(item) => output.push_str(&crate::ui::interactive::render_transcript_item(
                crate::ui::interactive::TranscriptRenderInput {
                    item: &item,
                    theme,
                    width: 80,
                    tools_expanded: false,
                    hide_thinking: false,
                },
            )),
            UiEvent::Output(OutputEvent::Text(text)) => output.push_str(&text),
            _ => {}
        }
    }
    output
}

#[test]
fn finished_bash_block_includes_elapsed_duration() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.finish_tool_line(ToolLine {
        name: "bash".to_string(),
        arguments: serde_json::json!({"command": "cargo test --all-targets"}),
        is_error: false,
        output: "test result: ok".to_string(),
        output_summary: "test result: ok".to_string(),
        duration_ms: Some(5000),
    });

    let output = collect_rendered_transcript(&mut events, &renderer.theme);
    assert!(output.contains("cargo test --all-targets"));
    assert!(output.contains("Took 5s"));
}

#[test]
fn finished_read_block_omits_elapsed_duration() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.finish_tool_line(ToolLine {
        name: "read".to_string(),
        arguments: serde_json::json!({"path": "src/main.rs"}),
        is_error: false,
        output: "hello world".to_string(),
        output_summary: "hello world".to_string(),
        duration_ms: Some(50),
    });

    let output = collect_rendered_transcript(&mut events, &renderer.theme);
    assert!(output.contains("read") && output.contains("src/main.rs"));
    assert!(!output.contains("Took"));
}

#[test]
fn finished_read_block_includes_line_range_styling() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.finish_tool_line(ToolLine {
        name: "read".to_string(),
        arguments: serde_json::json!({"path": "src/lib.rs", "offset": 10, "limit": 20}),
        is_error: false,
        output: "".to_string(),
        output_summary: "".to_string(),
        duration_ms: None,
    });

    let output = collect_rendered_transcript(&mut events, &renderer.theme);
    assert!(output.contains("read") && output.contains("src/lib.rs") && output.contains(":10-29"));
}
