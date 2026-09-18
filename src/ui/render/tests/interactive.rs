use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InteractiveUi, OutputEvent, UiEvent};
use rho_harness_core::presentation::ToolLine;

fn drain_interactive_output(
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
            UiEvent::Output(OutputEvent::Text(text) | OutputEvent::StreamText(text)) => output.push_str(&text),
            _ => {}
        }
    }
    (activity_events, output)
}

fn drain_text_output(events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>) -> String {
    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        if let UiEvent::Output(OutputEvent::Text(text) | OutputEvent::StreamText(text)) = event {
            output.push_str(&text);
        }
    }
    output
}

#[test]
fn interactive_renderer_marks_assistant_tokens_as_stream_output() {
    let (ui, mut events) = InteractiveUi::channel();
    let renderer = TerminalRenderer::with_ui(ui);

    renderer.print_token("answer");

    assert!(matches!(
        events.try_recv(),
        Ok(UiEvent::Output(OutputEvent::StreamText(text))) if text == "answer"
    ));
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

    let (activity_events, output) = drain_interactive_output(&mut events, &renderer.theme);
    assert_eq!(activity_events, [Activity::Thinking, Activity::Idle]);
    for token in ["considering", "answer", "read", "src/lib.rs"] {
        assert!(output.contains(token));
    }
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
    tool: &str,
    args: &serde_json::Value,
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
        "edit",
        &serde_json::json!({"path": "src/main.rs"}),
    );
    assert_fast_tool_run(
        &renderer,
        &mut events,
        "write",
        &serde_json::json!({"path": "src/main.rs", "content": "..."}),
    );
    assert_fast_tool_run(
        &renderer,
        &mut events,
        "read",
        &serde_json::json!({"path": "src/main.rs"}),
    );
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

    let (_, output) = drain_interactive_output(&mut events, &renderer.theme);
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

    let (_, output) = drain_interactive_output(&mut events, &renderer.theme);
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

    let (_, output) = drain_interactive_output(&mut events, &renderer.theme);
    assert!(output.contains("read") && output.contains("src/lib.rs") && output.contains(":10-29"));
}

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

    let (_, output) = drain_interactive_output(&mut events, &renderer.theme);
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

    let (_, output) = drain_interactive_output(&mut events, &renderer.theme);
    assert!(output.contains("web_search"));
    assert!(output.contains("\"serde release\""));
}
