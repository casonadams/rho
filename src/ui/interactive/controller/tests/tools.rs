//! Running tool widget lifecycle and transcript commit tests.

use super::fake::{FakeTerminal, Operation, SharedOperations};
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::{InteractiveState, ToolItem, ToolStartRequest, TranscriptItem, tool_fixture};

fn new_controller() -> (TerminalController<FakeTerminal>, SharedOperations) {
    let (backend, operations, _) = FakeTerminal::new(60);
    let controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    (controller, operations)
}

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
    tool_fixture("bash", serde_json::json!({"command": cmd}), out)
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

fn has_write(operations: &SharedOperations, pred: impl Fn(&str) -> bool) -> bool {
    operations
        .borrow()
        .iter()
        .any(|op| matches!(op, Operation::Write(text) if pred(text.as_str())))
}

#[test]
fn active_tool_status_updates_and_cleans_up_on_end() {
    let (mut controller, operations) = new_controller();
    operations.borrow_mut().clear();

    start_bash(&mut controller, "cargo test");
    assert_eq!(controller.state().footer().running_tool.as_deref(), Some("bash"));
    let writes = operations_writes(&operations.borrow());
    assert!(writes.contains("working") && writes.contains("bash") && writes.contains("cargo test"));

    operations.borrow_mut().clear();
    controller.end_tool().unwrap();
    assert_eq!(controller.state().footer().running_tool, None);
    let writes_after = operations_writes(&operations.borrow());
    assert!(!writes_after.contains("bash") && !writes_after.contains("cargo test"));
}

#[test]
fn active_tool_chunks_accumulate_in_state() {
    let (mut controller, _) = new_controller();

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
    let (mut controller, operations) = new_controller();

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

#[test]
fn tool_completion_in_place_preserves_editor_row_and_output_continuity() {
    let (mut controller, operations) = new_controller();

    start_bash(&mut controller, "cargo test");
    controller.append_tool_chunk("running 1 test\n").unwrap();
    operations.borrow_mut().clear();

    controller.push_transcript_item(bash_item("cargo test", "ok")).unwrap();
    assert!(controller.state().active_tool().is_none() && controller.transcript().len() == 1);

    let rendered = controller.rendered.as_ref().expect("rendered layout exists");
    assert_eq!((rendered.cursor_row(), rendered.lines.len()), (2, 6));

    controller.redraw().unwrap();
    let rendered_after_redraw = controller.rendered.as_ref().expect("rendered layout exists");
    assert_eq!(
        (rendered_after_redraw.cursor_row(), rendered_after_redraw.lines.len()),
        (2, 6)
    );

    operations.borrow_mut().clear();
    controller.write_output("Done.\n").unwrap();
    assert!(has_write(&operations, |text| text.contains("Done.")));
}

#[test]
fn consecutive_tools_commit_in_place_without_cumulative_drift() {
    let (mut controller, _) = new_controller();

    start_bash(&mut controller, "echo one");
    controller.push_transcript_item(bash_item("echo one", "one")).unwrap();

    start_bash(&mut controller, "echo two");
    controller.push_transcript_item(bash_item("echo two", "two")).unwrap();

    assert_eq!(
        (
            controller.transcript().len(),
            controller.state().active_tool().is_none()
        ),
        (2, true)
    );
    let rendered = controller.rendered.as_ref().unwrap();
    assert_eq!((rendered.cursor_row(), rendered.lines.len()), (2, 6));
}

#[test]
fn fast_tool_without_active_card_uses_standard_push_path() {
    let (mut controller, operations) = new_controller();
    operations.borrow_mut().clear();

    controller
        .push_transcript_item(TranscriptItem::Tool(ToolItem {
            name: "read".into(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
            is_error: false,
            output: "fn main() {}".into(),
            output_summary: "1 line".into(),
            duration_ms: Some(2),
        }))
        .unwrap();

    assert_eq!(controller.transcript().len(), 1);
    assert!(has_write(&operations, |text| text.contains("read")));
}

#[test]
fn committed_tool_preserves_blank_line_separation_from_preceding_block() {
    let (mut controller, operations) = new_controller();

    controller
        .push_transcript_item(TranscriptItem::UserMessage("run tests".into()))
        .unwrap();

    start_bash(&mut controller, "cargo test");
    operations.borrow_mut().clear();

    controller.push_transcript_item(bash_item("cargo test", "ok")).unwrap();

    assert!(has_write(&operations, str::is_empty));

    let (mut controller, operations) = new_controller();
    controller.push_transcript_item(bash_item("echo 1", "1")).unwrap();
    operations.borrow_mut().clear();

    start_bash(&mut controller, "echo 2");
    assert!(has_write(&operations, str::is_empty));
}
