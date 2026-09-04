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
            UiEvent::Activity(_)
            | UiEvent::RunningTool(_)
            | UiEvent::ExtraStatus(_)
            | UiEvent::ToolStart(_)
            | UiEvent::ToolChunk { .. }
            | UiEvent::ToolEnd => {}
            UiEvent::Interaction { .. } => panic!("unexpected interaction"),
        }
    }
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
            UiEvent::Activity(_)
            | UiEvent::RunningTool(_)
            | UiEvent::ExtraStatus(_)
            | UiEvent::ToolStart(_)
            | UiEvent::ToolChunk { .. }
            | UiEvent::ToolEnd => {}
            UiEvent::Interaction { .. } => panic!("unexpected interaction"),
        }
    }
    assert!(output.contains("read"));
    assert!(output.contains("src/main.rs"));
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

    let mut output = String::new();
    while let Ok(event) = events.try_recv() {
        if let UiEvent::Transcript(item) = event {
            output.push_str(&crate::ui::interactive::render_transcript_item(
                crate::ui::interactive::TranscriptRenderInput {
                    item: &item,
                    theme: &renderer.theme,
                    width: 80,
                    tools_expanded: false,
                    hide_thinking: false,
                },
            ));
        }
    }
    assert!(output.contains("read"));
    assert!(output.contains("src/lib.rs"));
    assert!(output.contains(":10-29"));
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
