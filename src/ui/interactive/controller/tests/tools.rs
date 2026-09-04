use super::fake::{FakeTerminal, Operation};
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::{InteractiveState, ToolItem, ToolStartRequest, TranscriptItem};

#[test]
fn active_tool_status_updates_and_cleans_up_on_end() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller
        .start_tool(ToolStartRequest {
            name: "bash".into(),
            args_summary: "cargo test".into(),
            preview: None,
        })
        .unwrap();
    assert_eq!(controller.state().footer().running_tool.as_deref(), Some("bash"));
    let ops = operations.borrow();
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("Working...")))
    );
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("bash") && text.contains("cargo test")))
    );
    drop(ops);

    operations.borrow_mut().clear();
    controller.end_tool().unwrap();
    assert_eq!(controller.state().footer().running_tool, None);
    let ops = operations.borrow();
    assert!(
        !ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("bash") && text.contains("cargo test")))
    );
}

#[test]
fn consecutive_tools_are_separated_by_blank_line() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    controller
        .push_transcript_item(TranscriptItem::Tool(ToolItem {
            name: "bash".into(),
            arguments: serde_json::json!({"command": "echo 1"}),
            is_error: false,
            output: "1".into(),
            output_summary: "1".into(),
            duration_ms: Some(10),
        }))
        .unwrap();

    operations.borrow_mut().clear();

    controller
        .start_tool(ToolStartRequest {
            name: "bash".into(),
            args_summary: "echo 2".into(),
            preview: None,
        })
        .unwrap();

    let ops = operations.borrow();
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.is_empty())),
        "active tool should have a leading empty line to separate from preceding transcript"
    );
}

#[test]
fn active_tool_chunks_accumulate_in_state() {
    let (backend, _, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    controller
        .start_tool(ToolStartRequest {
            name: "bash".into(),
            args_summary: "cargo build".into(),
            preview: None,
        })
        .unwrap();

    assert!(controller.state().active_tool().is_some());
    assert_eq!(controller.state().active_tool().unwrap().name, "bash");
    assert_eq!(controller.state().active_tool().unwrap().args_summary, "cargo build");
    assert_eq!(controller.state().active_tool().unwrap().output, "");

    controller.append_tool_chunk("   Compiling rho v0.1.0\n").unwrap();
    controller
        .append_tool_chunks(["    Finished dev [unoptimized + debuginfo] target(s)\n"])
        .unwrap();

    let output = &controller.state().active_tool().unwrap().output;
    assert!(output.contains("Compiling rho"));
    assert!(output.contains("Finished dev"));

    controller.end_tool().unwrap();
    assert!(controller.state().active_tool().is_none());
}

#[test]
fn tool_transcript_push_clears_widget_and_commits_block_atomically() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    controller
        .start_tool(ToolStartRequest {
            name: "bash".into(),
            args_summary: "cargo test".into(),
            preview: None,
        })
        .unwrap();
    controller.append_tool_chunk("partial running output\n").unwrap();
    operations.borrow_mut().clear();

    controller
        .push_transcript_item(TranscriptItem::Tool(ToolItem {
            name: "bash".into(),
            arguments: serde_json::json!({"command": "cargo test"}),
            is_error: false,
            output: "all tests passed".into(),
            output_summary: "completed".into(),
            duration_ms: Some(50),
        }))
        .unwrap();

    assert!(controller.state().active_tool().is_none());
    assert_eq!(controller.transcript().len(), 1);
    let writes: Vec<String> = operations
        .borrow()
        .iter()
        .filter_map(|op| match op {
            Operation::Write(text) => Some(text.clone()),
            _ => None,
        })
        .collect();
    let committed = writes.join("");
    assert!(committed.contains("all tests passed"));
    assert!(committed.contains("Took"));
    assert!(!committed.contains("partial running output"));
}
