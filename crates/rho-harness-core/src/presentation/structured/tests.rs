use super::*;
use serde_json::json;

#[tokio::test]
async fn structured_presenter_records_events_in_sequence() {
    let recording = RecordingSink::new();
    let presenter = StructuredPresenter::recording(recording.clone());

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

    let events = recording.events();
    assert_eq!(events.len(), 10);
    assert_eq!(
        events[0],
        UiEvent::TurnStarted {
            prompt: "test prompt".to_string()
        }
    );
    assert_eq!(
        events[1],
        UiEvent::UserBlock {
            input: "test prompt".to_string()
        }
    );
    assert_eq!(
        events[2],
        UiEvent::ThinkingToken {
            token: "thinking...".to_string()
        }
    );
    assert_eq!(
        events[3],
        UiEvent::Token {
            token: "response token".to_string()
        }
    );
    assert_eq!(
        events[4],
        UiEvent::ActivityStarted {
            message: "loading".to_string()
        }
    );
    assert_eq!(events[5], UiEvent::ActivityFinished);
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
    assert_eq!(
        events[9],
        UiEvent::TurnCompleted {
            status: "completed".to_string()
        }
    );
}
