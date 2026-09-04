use super::super::fake::{FakeTerminal, Operation};
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::{InteractiveState, ToolItem, TranscriptItem};

#[test]
fn assistant_transcript_item_is_recorded_without_duplicate_write_output() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller
        .push_transcript_item(TranscriptItem::AssistantText("streamed response answer".into()))
        .unwrap();

    assert_eq!(controller.transcript().len(), 1);
    let ops = operations.borrow();
    assert!(
        !ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("streamed response answer"))),
        "pushing already-streamed assistant text should not write to output again"
    );
}

#[test]
fn full_redraw_reuses_cached_rendered_items_across_expansion_toggles() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    let tool = TranscriptItem::Tool(ToolItem {
        name: "read".into(),
        arguments: serde_json::json!({"path": "src/main.rs"}),
        is_error: false,
        output: "fn main() {}".into(),
        output_summary: "1 line".into(),
        duration_ms: None,
    });
    controller.push_transcript_item(tool).unwrap();
    assert_eq!(controller.cache().len(), 1);

    controller.toggle_tools_expanded().unwrap();
    let expanded_entry = controller.cache().entry(0).unwrap().clone();
    assert!(expanded_entry.standard.is_some());
    assert!(expanded_entry.alternate.is_some());

    controller.toggle_tools_expanded().unwrap();
    let collapsed_entry = controller.cache().entry(0).unwrap().clone();
    assert_eq!(expanded_entry, collapsed_entry);

    operations.borrow_mut().clear();
    controller.full_redraw().unwrap();
    let final_entry = controller.cache().entry(0).unwrap().clone();
    assert_eq!(collapsed_entry, final_entry);
}

#[test]
fn set_tools_expanded_no_ops_when_already_in_target_state() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    assert!(!controller.tools_expanded());

    operations.borrow_mut().clear();
    assert!(!controller.set_tools_expanded(false).unwrap());
    assert!(operations.borrow().is_empty());

    assert!(controller.set_tools_expanded(true).unwrap());
    assert!(controller.tools_expanded());

    operations.borrow_mut().clear();
    assert!(controller.set_tools_expanded(true).unwrap());
    assert!(operations.borrow().is_empty());
}

#[test]
fn set_hide_thinking_no_ops_when_already_in_target_state() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    assert!(!controller.hide_thinking());

    operations.borrow_mut().clear();
    assert!(!controller.set_hide_thinking(false).unwrap());
    assert!(operations.borrow().is_empty());

    assert!(controller.set_hide_thinking(true).unwrap());
    assert!(controller.hide_thinking());

    operations.borrow_mut().clear();
    assert!(controller.set_hide_thinking(true).unwrap());
    assert!(operations.borrow().is_empty());
}

#[test]
fn toggle_methods_delegate_cleanly_to_setters() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    operations.borrow_mut().clear();
    assert!(controller.toggle_tools_expanded().unwrap());
    assert!(controller.tools_expanded());
    assert!(!operations.borrow().is_empty());

    operations.borrow_mut().clear();
    assert!(!controller.toggle_tools_expanded().unwrap());
    assert!(!controller.tools_expanded());
    assert!(!operations.borrow().is_empty());

    operations.borrow_mut().clear();
    assert!(controller.toggle_thinking().unwrap());
    assert!(controller.hide_thinking());
    assert!(!operations.borrow().is_empty());

    operations.borrow_mut().clear();
    assert!(!controller.toggle_thinking().unwrap());
    assert!(!controller.hide_thinking());
    assert!(!operations.borrow().is_empty());
}
