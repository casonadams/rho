use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InteractiveUi, OutputEvent, UiEvent};
use rho_harness_core::presentation::ToolLine;

fn drain_interactive_events(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    theme: &crate::ui::theme::Theme,
) -> (Vec<Activity>, String) {
    let mut activity_events = Vec::new();
    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::Activity(activity) => activity_events.push(activity),
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
    (activity_events, output)
}

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

    let (activity_events, output) = drain_interactive_events(&mut events, &renderer.theme);
    assert_eq!(activity_events, [Activity::Thinking, Activity::Idle]);
    for token in ["considering", "answer", "read", "src/lib.rs"] {
        assert!(output.contains(token));
    }
}

fn drain_text_output(events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>) -> String {
    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        if let UiEvent::Output(OutputEvent::Text(text)) = event {
            output.push_str(&text);
        }
    }
    output
}

#[test]
fn renderer_flush_resets_markdown_state_between_turns() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.print_token("First response.\n");
    renderer.flush();
    while events.try_recv().is_ok() {}

    renderer.print_token("# Second Turn Title\n");
    renderer.flush();

    let turn2_output = drain_text_output(&mut events);
    assert!(
        !turn2_output.starts_with('\n'),
        "expected no extra leading newline, got: {turn2_output:?}"
    );
    assert!(turn2_output.contains("Second Turn Title"));
}

fn assert_fast_tool_run(
    renderer: &TerminalRenderer,
    events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    (tool, args): (&str, &serde_json::Value),
) {
    renderer.start_tool_run(tool, args);
    assert!(matches!(events.try_recv(), Ok(UiEvent::RunningTool(Some(name))) if name == tool));
    assert!(events.try_recv().is_err());
}

#[test]
fn fast_tools_emit_footer_status_without_live_widget_bounce() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    assert_fast_tool_run(
        &renderer,
        &mut events,
        ("edit", &serde_json::json!({"path": "src/main.rs"})),
    );
    assert_fast_tool_run(
        &renderer,
        &mut events,
        ("write", &serde_json::json!({"path": "src/main.rs", "content": "hello"})),
    );

    renderer.start_tool_run("bash", &serde_json::json!({"command": "cargo test"}));
    assert!(matches!(events.try_recv(), Ok(UiEvent::ToolStart(req)) if req.name == "bash"));
}
