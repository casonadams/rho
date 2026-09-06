mod commit;

use super::fake::{FakeTerminal, Operation};
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::{InteractiveState, ToolItem, ToolStartRequest, TranscriptItem};

fn start_bash(controller: &mut TerminalController<FakeTerminal>, cmd: &str) {
    controller
        .start_tool(ToolStartRequest {
            name: "bash".into(),
            args_summary: cmd.into(),
            preview: None,
        })
        .unwrap();
}

fn bash_item(cmd: &str, out: &str) -> TranscriptItem {
    TranscriptItem::Tool(ToolItem {
        name: "bash".into(),
        arguments: serde_json::json!({"command": cmd}),
        is_error: false,
        output: out.into(),
        output_summary: out.into(),
        duration_ms: Some(10),
    })
}

fn operations_writes(ops: &[Operation]) -> String {
    ops.iter()
        .filter_map(|op| match op {
            Operation::Write(t) => Some(t.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

#[test]
fn active_tool_status_updates_and_cleans_up_on_end() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    start_bash(&mut controller, "cargo test");
    assert_eq!(controller.state().footer().running_tool.as_deref(), Some("bash"));
    let writes = operations_writes(&operations.borrow());
    assert!(writes.contains("Working...") && writes.contains("bash") && writes.contains("cargo test"));

    operations.borrow_mut().clear();
    controller.end_tool().unwrap();
    assert_eq!(controller.state().footer().running_tool, None);
    let writes_after = operations_writes(&operations.borrow());
    assert!(!writes_after.contains("bash") && !writes_after.contains("cargo test"));
}

#[test]
fn consecutive_tools_are_separated_by_blank_line() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    controller.push_transcript_item(bash_item("echo 1", "1")).unwrap();
    operations.borrow_mut().clear();

    start_bash(&mut controller, "echo 2");
    let ops = operations.borrow();
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.is_empty()))
    );
}

#[test]
fn active_tool_chunks_accumulate_in_state() {
    let (backend, _, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    start_bash(&mut controller, "cargo build");
    let tool = controller.state().active_tool().unwrap();
    assert_eq!(
        (tool.name.as_str(), tool.args_summary.as_str(), tool.output.as_str()),
        ("bash", "cargo build", "")
    );

    controller.append_tool_chunk("   Compiling rho v0.1.0\n").unwrap();
    controller
        .append_tool_chunks(["    Finished dev [unoptimized + debuginfo] target(s)\n"])
        .unwrap();

    let output = &controller.state().active_tool().unwrap().output;
    assert!(output.contains("Compiling rho") && output.contains("Finished dev"));
    controller.end_tool().unwrap();
    assert!(controller.state().active_tool().is_none());
}

#[test]
fn tool_transcript_push_clears_widget_and_commits_block_atomically() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    start_bash(&mut controller, "cargo test");
    controller.append_tool_chunk("partial running output\n").unwrap();
    operations.borrow_mut().clear();

    controller
        .push_transcript_item(bash_item("cargo test", "all tests passed"))
        .unwrap();
    assert!(controller.state().active_tool().is_none() && controller.transcript().len() == 1);

    let committed = operations_writes(&operations.borrow());
    assert!(committed.contains("all tests passed") && !committed.contains("partial running output"));
}
