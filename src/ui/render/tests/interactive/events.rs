use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InteractiveUi, OutputEvent, UiEvent};
use rho_harness_core::presentation::ToolLine;

#[test]
fn interactive_renderer_emits_formatted_output_and_activity_events() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    let activity = renderer.start_spinner("thinking...");
    renderer.print_thinking_token("considering");
    activity.finish_and_clear();
    renderer.print_token("answer");
    renderer.flush();
    renderer.finish_tool_line(ToolLine {
        name: "read".to_string(),
        arguments: serde_json::json!({"path": "src/lib.rs"}),
        is_error: false,
        output: "contents".to_string(),
        output_summary: "contents".to_string(),
        duration_ms: None,
    });

    let mut activity_events = Vec::new();
    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Activity(activity) => activity_events.push(activity),
            UiEvent::Transcript(item) => output.push_str(&crate::ui::interactive::render_transcript_item(
                crate::ui::interactive::TranscriptRenderInput {
                    item: &item,
                    theme: &renderer.theme,
                    width: 80,
                    tools_expanded: false,
                    hide_thinking: false,
                },
            )),
            UiEvent::RunningTool(_)
            | UiEvent::ExtraStatus(_)
            | UiEvent::ToolStart(_)
            | UiEvent::ToolChunk { .. }
            | UiEvent::ToolEnd => {}
            UiEvent::Output(OutputEvent::Text(text)) => output.push_str(&text),
            UiEvent::Interaction { .. } => panic!("unexpected interaction"),
        }
    }
    assert_eq!(activity_events, [Activity::Thinking, Activity::Idle]);
    assert!(output.contains("considering"));
    assert!(output.contains("answer"));
    assert!(output.contains("read"));
    assert!(output.contains("src/lib.rs"));
}
