use super::*;
use serde_json::json;

fn emit_test_events(presenter: &StructuredPresenter) {
    presenter.print_turn_started("test prompt");
    presenter.print_user_block("test prompt");
    presenter.print_thinking_token("thinking...");
    presenter.print_token("response token");
    let spinner = presenter.start_spinner("loading");
    spinner.finish_and_clear();
    presenter.start_tool_run("bash", &json!({"command": "ls"}));
    presenter.stream_port().stream_chunk("file.txt\n");
    presenter.finish_tool_line(ToolLine {
        name: "bash".to_string(),
        arguments: json!({"command": "ls"}),
        is_error: false,
        output: "file.txt\n".to_string(),
        output_summary: "file.txt".to_string(),
        duration_ms: Some(10),
    });
    presenter.print_turn_completed("completed");
}

fn expected_initial_events() -> [UiEvent; 6] {
    [
        UiEvent::TurnStarted {
            prompt: "test prompt".to_string(),
        },
        UiEvent::UserBlock {
            input: "test prompt".to_string(),
        },
        UiEvent::ThinkingToken {
            token: "thinking...".to_string(),
        },
        UiEvent::Token {
            token: "response token".to_string(),
        },
        UiEvent::ActivityStarted {
            message: "loading".to_string(),
        },
        UiEvent::ActivityFinished,
    ]
}

fn assert_initial_events(events: &[UiEvent]) {
    assert_eq!(&events[0..6], &expected_initial_events());
}

fn assert_tool_events(events: &[UiEvent]) {
    assert_eq!(
        events[6],
        UiEvent::ToolStarted {
            name: "bash".to_string(),
            arguments: json!({"command": "ls"})
        }
    );
    assert_eq!(
        events[7],
        UiEvent::ToolChunk {
            name: String::new(),
            chunk: "file.txt\n".to_string()
        }
    );
    assert!(matches!(events[8], UiEvent::ToolFinished { .. }));
}

fn assert_turn_completed(events: &[UiEvent]) {
    assert_eq!(
        events[9],
        UiEvent::TurnCompleted {
            status: "completed".to_string()
        }
    );
}

#[tokio::test]
async fn structured_presenter_records_events_in_sequence() {
    let recording = RecordingSink::new();
    let presenter = StructuredPresenter::recording(recording.clone());
    emit_test_events(&presenter);
    let events = recording.events();
    assert_initial_events(&events);
    assert_tool_events(&events);
    assert_turn_completed(&events);
}
