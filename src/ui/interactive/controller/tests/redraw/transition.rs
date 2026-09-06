use super::super::fake::{FakeTerminal, Operation};
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::{InteractiveState, ModalOption, ModalState};

fn track_cursor_ops(ops: &[Operation], initial_row: usize) -> (isize, isize, usize) {
    let mut cursor_row = initial_row as isize;
    let mut max_row = cursor_row;
    let mut last_col = 0;
    for op in ops {
        match op {
            Operation::Up(n) => cursor_row -= *n as isize,
            Operation::Down(n) => cursor_row += *n as isize,
            Operation::Column(c) => last_col = *c,
            Operation::Write(text) => cursor_row += text.matches("\r\n").count() as isize,
            _ => {}
        }
        max_row = max_row.max(cursor_row);
    }
    (max_row, cursor_row, last_col)
}

fn setup_transition_controller(
    modal: ModalState,
) -> (
    TerminalController<FakeTerminal>,
    std::rc::Rc<std::cell::RefCell<Vec<Operation>>>,
    usize,
    usize,
) {
    let (backend, operations, _width, _height) = FakeTerminal::with_size(80, 15);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
    controller.state_mut().push_modal(modal);
    controller.redraw().unwrap();
    let layout = controller.current_layout();
    assert!(layout.height() <= 15);
    let (height, cursor_row) = (layout.height(), layout.cursor_row());
    operations.borrow_mut().clear();
    (controller, operations, height, cursor_row)
}

#[test]
fn modal_dismissal_to_editor_clears_excess_lines_and_synchronizes_cursor() {
    let body = (1..=30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let modal = ModalState::new(
        "Perm",
        &body,
        vec![ModalOption::from("Allow"), ModalOption::from("Deny")],
    );
    let (mut controller, operations, modal_height, modal_cursor_row) = setup_transition_controller(modal);

    controller.state_mut().pop_modal();
    controller.redraw().unwrap();

    let editor_layout = controller.current_layout();
    let ops = operations.borrow();
    assert!(ops.iter().filter(|op| **op == Operation::Clear).count() >= modal_height);
    let (max_row, cursor_row, last_col) = track_cursor_ops(&ops, modal_cursor_row);

    assert!(max_row < 15);
    assert_eq!(cursor_row, editor_layout.cursor_row() as isize);
    assert_eq!(last_col, editor_layout.cursor.column);
}

#[test]
fn searchable_modal_dismissal_synchronizes_cursor() {
    let body = (1..=20).map(|i| format!("desc {i}")).collect::<Vec<_>>().join("\n");
    let modal = ModalState::new("Select", &body, vec![ModalOption::from("Item A")]).with_search(true);
    let (mut controller, operations, _height, modal_cursor_row) = setup_transition_controller(modal);

    controller.state_mut().pop_modal();
    controller.redraw().unwrap();

    let editor_layout = controller.current_layout();
    let ops = operations.borrow();
    let (_, cursor_row, last_col) = track_cursor_ops(&ops, modal_cursor_row);

    assert_eq!(cursor_row, editor_layout.cursor_row() as isize);
    assert_eq!(last_col, editor_layout.cursor.column);
}

fn sample_perm_options() -> Vec<ModalOption> {
    vec![
        ModalOption::from("Allow"),
        ModalOption::from("Edit"),
        ModalOption::from("Always"),
        ModalOption::from("Deny"),
    ]
}

#[test]
fn modal_redraw_ticks_and_navigation_preserve_zero_scrollback_pollution() {
    let body = (1..=10).map(|i| format!("cmd {i}")).collect::<Vec<_>>().join("\n");
    let mut modal = ModalState::new("Permission Required", &body, sample_perm_options());
    modal.option_layout = crate::ui::interactive::OptionLayout::Horizontal;
    let (mut controller, operations, modal_height, _) = setup_transition_controller(modal);

    assert_eq!(
        controller.rendered().unwrap().lines[0],
        controller.rendered().unwrap().top_divider
    );
    for _ in 0..10 {
        controller.state_mut().select_next_modal_option();
        controller.redraw().unwrap();
        assert_eq!(controller.rendered().unwrap().height(), modal_height);
    }
    let ops = operations.borrow();
    assert!(
        !ops.iter()
            .any(|op| matches!(op, Operation::Write(s) if s.contains("\r\n")))
    );
}
