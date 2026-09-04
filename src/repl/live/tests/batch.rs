use super::common::HistoryTerminal;
use crate::ui::interactive::{InteractiveState, TerminalController};

#[test]
fn live_batch_flushes_tool_end_with_transcript_without_intermediate_redraw() {
    let mut batch = super::super::batch::LiveBatch::new();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller
        .start_tool(crate::ui::interactive::ToolStartRequest {
            name: "bash".into(),
            args_summary: "cargo test".into(),
            preview: None,
        })
        .unwrap();

    batch
        .enqueue(&mut controller, crate::ui::interactive::UiEvent::ToolEnd)
        .unwrap();
    batch
        .enqueue(
            &mut controller,
            crate::ui::interactive::UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Tool(
                crate::ui::interactive::ToolItem {
                    name: "bash".into(),
                    arguments: serde_json::json!({"command": "cargo test"}),
                    is_error: false,
                    output: "all tests passed".into(),
                    output_summary: "ok".into(),
                    duration_ms: Some(50),
                },
            )),
        )
        .unwrap();

    batch.flush(&mut controller, false).unwrap();
    assert_eq!(controller.transcript().len(), 1);
}

#[test]
fn live_batch_coalesces_rapid_tool_activity_and_transcript() {
    let mut batch = super::super::batch::LiveBatch::new();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller
        .start_tool(crate::ui::interactive::ToolStartRequest {
            name: "read".into(),
            args_summary: "src/main.rs".into(),
            preview: None,
        })
        .unwrap();

    assert!(
        !batch
            .push_event(
                &mut controller,
                crate::ui::interactive::UiEvent::Activity(crate::ui::interactive::Activity::Working)
            )
            .unwrap()
    );
    assert!(
        !batch
            .push_event(
                &mut controller,
                crate::ui::interactive::UiEvent::RunningTool(Some("read".into()))
            )
            .unwrap()
    );
    assert!(
        !batch
            .push_event(
                &mut controller,
                crate::ui::interactive::UiEvent::Activity(crate::ui::interactive::Activity::Idle)
            )
            .unwrap()
    );
    assert!(
        batch
            .push_event(
                &mut controller,
                crate::ui::interactive::UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Tool(
                    crate::ui::interactive::ToolItem {
                        name: "read".into(),
                        arguments: serde_json::json!({"path": "src/main.rs"}),
                        is_error: false,
                        output: "fn main() {}".into(),
                        output_summary: "fn main() {}".into(),
                        duration_ms: Some(1),
                    },
                )),
            )
            .unwrap()
    );
    assert!(
        !batch
            .push_event(
                &mut controller,
                crate::ui::interactive::UiEvent::Activity(crate::ui::interactive::Activity::Thinking)
            )
            .unwrap()
    );

    batch.flush(&mut controller, false).unwrap();
    assert_eq!(controller.transcript().len(), 1);
    assert!(matches!(
        controller.state().footer().activity,
        crate::ui::interactive::Activity::Thinking
    ));
    assert_eq!(controller.state().footer().running_tool, None);
    assert!(controller.state().active_tool().is_none());
}
