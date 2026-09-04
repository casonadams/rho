use super::fake::{FakeTerminal, Operation};
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::controller::ansi::{CSI_BEGIN_SYNC_UPDATE, CSI_END_SYNC_UPDATE};
use crate::ui::interactive::{InteractiveState, ToolItem, TranscriptItem};

#[test]
fn full_redraw_rerenders_all_transcript_items_on_resize() {
    let (backend, operations, width) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    controller
        .push_transcript_item(TranscriptItem::UserMessage("hello world message".into()))
        .unwrap();
    operations.borrow_mut().clear();

    width.set(40);
    assert!(controller.refresh_size().unwrap());

    let ops = operations.borrow();
    assert!(ops.contains(&Operation::Write("\x1b[2J\x1b[H\x1b[3J".into())));
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("hello world message")))
    );
}

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
fn full_redraw_emits_synchronized_update_escape_codes_and_batches_output() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    controller
        .push_transcript_item(TranscriptItem::UserMessage("first line message".into()))
        .unwrap();
    controller
        .push_transcript_item(TranscriptItem::UserMessage("second line message".into()))
        .unwrap();
    operations.borrow_mut().clear();

    controller.full_redraw().unwrap();

    let ops = operations.borrow();
    let sync_begin_pos = ops
        .iter()
        .position(|op| matches!(op, Operation::Write(text) if text == CSI_BEGIN_SYNC_UPDATE))
        .expect("CSI 2026h must be emitted");
    let clear_pos = ops
        .iter()
        .position(|op| matches!(op, Operation::Write(text) if text == "\x1b[2J\x1b[H\x1b[3J"))
        .expect("screen clear must be emitted");
    let sync_end_pos = ops
        .iter()
        .position(|op| matches!(op, Operation::Write(text) if text == CSI_END_SYNC_UPDATE))
        .expect("CSI 2026l must be emitted");
    let flush_pos = ops
        .iter()
        .position(|op| matches!(op, Operation::Flush))
        .expect("flush must be emitted");

    assert!(sync_begin_pos < clear_pos, "sync update begin must precede clear");
    assert!(clear_pos < sync_end_pos, "clear must precede sync update end");
    assert!(sync_end_pos < flush_pos, "sync update end must precede flush");

    let batched_writes: Vec<_> = ops
        .iter()
        .filter(|op| {
            matches!(
                op,
                Operation::Write(text) if text.contains("first line message") && text.contains("second line message")
            )
        })
        .collect();
    assert_eq!(
        batched_writes.len(),
        1,
        "all transcript items must be batched in a single write"
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
fn width_resize_invalidates_cache_while_height_resize_preserves_cache() {
    let (backend, _, width, height) = FakeTerminal::with_size(60, 24);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    controller
        .push_transcript_item(TranscriptItem::UserMessage("test message".into()))
        .unwrap();

    let initial_rendered = controller.cache().entry(0).unwrap().standard.clone().unwrap();

    height.set(30);
    assert!(controller.refresh_size().unwrap());
    let height_rendered = controller.cache().entry(0).unwrap().standard.clone().unwrap();
    assert_eq!(initial_rendered, height_rendered);

    width.set(40);
    assert!(controller.refresh_size().unwrap());
    let width_rendered = controller.cache().entry(0).unwrap().standard.clone().unwrap();
    assert_ne!(initial_rendered, width_rendered);
}
