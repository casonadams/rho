use std::{
    cell::{Cell, RefCell},
    io,
    rc::Rc,
};

use super::{TerminalBackend, TerminalController, output_cursor};
use crate::ui::interactive::{Activity, InteractiveState, OutputEvent, PendingUiBatch, UiEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Operation {
    Raw(bool),
    Size,
    Hide,
    Show,
    Up(usize),
    Down(usize),
    Column(usize),
    Clear,
    Write(String),
    Flush,
}

type SharedOperations = Rc<RefCell<Vec<Operation>>>;
type SharedWidth = Rc<Cell<u16>>;

struct FakeTerminal {
    operations: SharedOperations,
    width: SharedWidth,
    fail_write: bool,
}

impl FakeTerminal {
    fn new(width: u16) -> (Self, SharedOperations, SharedWidth) {
        let operations = Rc::new(RefCell::new(Vec::new()));
        let width = Rc::new(Cell::new(width));
        (
            Self {
                operations: Rc::clone(&operations),
                width: Rc::clone(&width),
                fail_write: false,
            },
            operations,
            width,
        )
    }
}

impl TerminalBackend for FakeTerminal {
    fn set_raw_mode(&mut self, enabled: bool) -> io::Result<()> {
        self.operations.borrow_mut().push(Operation::Raw(enabled));
        Ok(())
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        self.operations.borrow_mut().push(Operation::Size);
        Ok((self.width.get(), 24))
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.operations.borrow_mut().push(Operation::Hide);
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.operations.borrow_mut().push(Operation::Show);
        Ok(())
    }

    fn move_up(&mut self, rows: usize) -> io::Result<()> {
        self.operations.borrow_mut().push(Operation::Up(rows));
        Ok(())
    }

    fn move_down(&mut self, rows: usize) -> io::Result<()> {
        self.operations.borrow_mut().push(Operation::Down(rows));
        Ok(())
    }

    fn move_to_column(&mut self, column: usize) -> io::Result<()> {
        self.operations.borrow_mut().push(Operation::Column(column));
        Ok(())
    }

    fn clear_line(&mut self) -> io::Result<()> {
        self.operations.borrow_mut().push(Operation::Clear);
        Ok(())
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        self.operations.borrow_mut().push(Operation::Write(text.to_string()));
        if self.fail_write {
            Err(io::Error::other("write failed"))
        } else {
            Ok(())
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.operations.borrow_mut().push(Operation::Flush);
        Ok(())
    }
}

#[test]
fn output_cursor_tracks_wrap_boundaries_styles_and_wide_text() {
    assert_eq!(output_cursor("123456789", 10), (9, false));
    assert_eq!(output_cursor("1234567890", 10), (0, true));
    assert_eq!(output_cursor("123456789界", 10), (2, false));
    assert_eq!(output_cursor("\u{1b}[2mwide\u{1b}[0m", 10), (4, false));
}

#[test]
fn construction_positions_and_shows_the_editor_cursor() {
    let (backend, operations, _) = FakeTerminal::new(10);

    let _controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    let operations = operations.borrow();
    let show_index = operations
        .iter()
        .rposition(|operation| operation == &Operation::Show)
        .unwrap();
    let flush_index = operations
        .iter()
        .rposition(|operation| operation == &Operation::Flush)
        .unwrap();
    assert!(operations[..show_index].contains(&Operation::Hide));
    assert!(show_index < flush_index);
}

#[test]
fn output_erases_then_writes_then_redraws_with_one_flush() {
    let (backend, operations, _) = FakeTerminal::new(10);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller.write_output("answer\nnext").unwrap();

    let operations = operations.borrow();
    let output_index = operations
        .iter()
        .position(|operation| operation == &Operation::Write("answer\r\nnext".into()))
        .unwrap();
    let last_clear = operations
        .iter()
        .rposition(|operation| operation == &Operation::Clear)
        .unwrap();
    let divider_index = operations
        .iter()
        .position(|operation| matches!(operation, Operation::Write(text) if text.contains("──────────")))
        .unwrap();
    assert!(last_clear < output_index);
    assert!(output_index < divider_index);
    assert_eq!(
        operations
            .iter()
            .filter(|operation| operation == &&Operation::Flush)
            .count(),
        1
    );
}

#[test]
fn many_stream_fragments_are_written_with_one_controller_flush() {
    let (backend, operations, _) = FakeTerminal::new(40);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();
    let mut pending = PendingUiBatch::new(16 * 1024);
    for _ in 0..1_000 {
        pending.push(UiEvent::Output(OutputEvent::Text("token".into())));
    }

    controller.write_output(&pending.drain().text).unwrap();

    let operations = operations.borrow();
    assert_eq!(
        operations
            .iter()
            .filter(|operation| operation == &&Operation::Write("token".repeat(1_000)))
            .count(),
        1
    );
    assert_eq!(
        operations
            .iter()
            .filter(|operation| operation == &&Operation::Flush)
            .count(),
        1
    );
}

#[test]
fn streamed_output_resumes_at_the_previous_line_end() {
    let (backend, operations, _) = FakeTerminal::new(10);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller.write_output("streamed ").unwrap();
    operations.borrow_mut().clear();
    controller.write_output("response").unwrap();

    let operations = operations.borrow();
    let move_index = operations
        .iter()
        .position(|operation| operation == &Operation::Up(1))
        .unwrap();
    let column_index = operations
        .iter()
        .position(|operation| operation == &Operation::Column(9))
        .unwrap();
    let output_index = operations
        .iter()
        .position(|operation| operation == &Operation::Write("response".into()))
        .unwrap();
    assert!(move_index < column_index);
    assert!(column_index < output_index);
}

#[test]
fn resize_erases_using_old_layout_and_redraws_at_new_width() {
    let (backend, operations, width) = FakeTerminal::new(8);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();
    width.set(4);

    assert!(controller.refresh_size().unwrap());

    let operations = operations.borrow();
    let clear_index = operations
        .iter()
        .position(|operation| operation == &Operation::Clear)
        .unwrap();
    let divider_index = operations
        .iter()
        .position(|operation| matches!(operation, Operation::Write(text) if text.contains("────")))
        .unwrap();
    assert!(clear_index < divider_index);
}

#[test]
fn resize_rerenders_at_new_width() {
    let (backend, operations, width) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    controller
        .start_tool(crate::ui::interactive::ToolStartRequest {
            name: "bash".into(),
            args_summary: "cargo test".into(),
            preview: None,
        })
        .unwrap();
    operations.borrow_mut().clear();
    width.set(30);

    assert!(controller.refresh_size().unwrap());

    let ops = operations.borrow();
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("Working...")))
    );
}

#[test]
fn tick_redraws_the_live_region() {
    let (backend, operations, _) = FakeTerminal::new(8);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    let operations = operations.borrow();
    assert!(operations.contains(&Operation::Clear));
    assert!(
        operations
            .iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("────────")))
    );
    assert!(operations.ends_with(&[Operation::Show, Operation::Flush]));
}

#[test]
fn busy_working_line_renders_above_the_editor() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut state = InteractiveState::default();
    state.footer_mut().activity = Activity::Thinking;
    let mut controller = TerminalController::new(backend, state).unwrap();
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    let ops = operations.borrow();
    let working_index = ops.iter().position(|op| {
        matches!(
            op,
            Operation::Write(text) if text.contains("Thinking...") || text.contains("Working...")
        )
    });
    let divider_index = ops
        .iter()
        .position(|op| matches!(op, Operation::Write(text) if text.contains(&"\u{2500}".repeat(60))));

    assert!(working_index.is_some());
    assert!(working_index.unwrap() < divider_index.unwrap());
}

#[test]
fn busy_working_line_disappears_when_idle() {
    let (backend, operations, _) = FakeTerminal::new(20);
    let mut state = InteractiveState::default();
    state.footer_mut().activity = Activity::Working;
    let mut controller = TerminalController::new(backend, state).unwrap();
    controller.state_mut().footer_mut().activity = Activity::Idle;
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    let ops = operations.borrow();
    assert!(
        !ops.iter().any(
            |op| matches!(op, Operation::Write(text) if text.contains("Working...") || text.contains("Thinking..."))
        )
    );
}

#[test]
fn footer_carries_no_spinner_or_activity_label_when_busy() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut state = InteractiveState::default();
    state.footer_mut().activity = Activity::Working;
    state.footer_mut().model = "model".into();
    let mut controller = TerminalController::new(backend, state).unwrap();
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    let ops = operations.borrow();
    assert!(
        ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("model") && text.contains("\u{1b}[2m")))
    );
    assert!(
        !ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("working")))
    );
    assert!(
        !ops.iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("thinking")))
    );
}

#[test]
fn idle_footer_is_rendered_dimmed() {
    let (backend, operations, _) = FakeTerminal::new(20);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller.tick().unwrap();

    assert!(
        operations
            .borrow()
            .iter()
            .any(|op| matches!(op, Operation::Write(text) if text.contains("\u{1b}[2m")))
    );
}

#[test]
fn unchanged_size_does_not_redraw() {
    let (backend, operations, _) = FakeTerminal::new(8);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    assert!(!controller.refresh_size().unwrap());
    assert_eq!(*operations.borrow(), [Operation::Size]);
}

#[test]
fn suspend_and_resume_restore_terminal_modes_around_legacy_prompts() {
    let (backend, operations, _) = FakeTerminal::new(8);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller.suspend().unwrap();
    assert!(
        operations
            .borrow()
            .ends_with(&[Operation::Show, Operation::Raw(false), Operation::Flush,])
    );
    operations.borrow_mut().clear();
    controller.resume().unwrap();
    assert_eq!(operations.borrow().first(), Some(&Operation::Raw(true)));
    assert!(operations.borrow().ends_with(&[Operation::Show, Operation::Flush]));
}

#[test]
fn active_tool_status_updates_and_cleans_up_on_end() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller
        .start_tool(crate::ui::interactive::ToolStartRequest {
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
fn full_redraw_rerenders_all_transcript_items_on_resize() {
    let (backend, operations, width) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    controller
        .push_transcript_item(crate::ui::interactive::TranscriptItem::UserMessage(
            "hello world message".into(),
        ))
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
fn drop_erases_region_and_restores_terminal() {
    let (backend, operations, _) = FakeTerminal::new(8);
    let controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    drop(controller);

    let operations = operations.borrow();
    assert!(operations.contains(&Operation::Clear));
    assert!(operations.ends_with(&[Operation::Show, Operation::Raw(false), Operation::Flush,]));
}

#[test]
fn construction_error_restores_cursor_and_raw_mode() {
    let (mut backend, operations, _) = FakeTerminal::new(8);
    backend.fail_write = true;

    assert!(TerminalController::new(backend, InteractiveState::default()).is_err());

    let operations = operations.borrow();
    assert!(operations.contains(&Operation::Show));
    assert!(operations.contains(&Operation::Raw(false)));
}

#[test]
fn assistant_transcript_item_is_recorded_without_duplicate_write_output() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    operations.borrow_mut().clear();

    controller
        .push_transcript_item(crate::ui::interactive::TranscriptItem::AssistantText(
            "streamed response answer".into(),
        ))
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
fn consecutive_tools_are_separated_by_blank_line() {
    let (backend, operations, _) = FakeTerminal::new(60);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    // First tool completes and is recorded in transcript
    controller
        .push_transcript_item(crate::ui::interactive::TranscriptItem::Tool(
            crate::ui::interactive::ToolItem {
                name: "bash".into(),
                arguments: serde_json::json!({"command": "echo 1"}),
                is_error: false,
                output: "1".into(),
                output_summary: "1".into(),
                duration_ms: Some(10),
            },
        ))
        .unwrap();

    operations.borrow_mut().clear();

    // Second tool starts running
    controller
        .start_tool(crate::ui::interactive::ToolStartRequest {
            name: "bash".into(),
            args_summary: "echo 2".into(),
            preview: None,
        })
        .unwrap();

    let ops = operations.borrow();
    // Verify that active tool is drawn with a leading newline separation
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
        .start_tool(crate::ui::interactive::ToolStartRequest {
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
        .start_tool(crate::ui::interactive::ToolStartRequest {
            name: "bash".into(),
            args_summary: "cargo test".into(),
            preview: None,
        })
        .unwrap();
    controller.append_tool_chunk("partial running output\n").unwrap();
    operations.borrow_mut().clear();

    controller
        .push_transcript_item(crate::ui::interactive::TranscriptItem::Tool(
            crate::ui::interactive::ToolItem {
                name: "bash".into(),
                arguments: serde_json::json!({"command": "cargo test"}),
                is_error: false,
                output: "all tests passed".into(),
                output_summary: "completed".into(),
                duration_ms: Some(50),
            },
        ))
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
