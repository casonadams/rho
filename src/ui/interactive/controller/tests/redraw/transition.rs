use super::super::fake::{FakeTerminal, Operation};
use crate::ui::interactive::controller::TerminalController;
use crate::ui::interactive::{InteractiveState, ModalOption, ModalState};

#[test]
fn modal_dismissal_to_editor_clears_excess_lines_and_synchronizes_cursor() {
    let (backend, operations, _width, _height) = FakeTerminal::with_size(80, 15);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    let body = (1..=30)
        .map(|i| format!("command argument line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let modal = ModalState::new(
        "Permission Required",
        &body,
        vec![ModalOption::from("Allow"), ModalOption::from("Deny")],
    );
    controller.state_mut().push_modal(modal);
    controller.redraw().unwrap();

    let modal_layout = controller.current_layout();
    assert!(modal_layout.height() <= 15);
    let modal_cursor_row = modal_layout.cursor_row();

    operations.borrow_mut().clear();
    controller.state_mut().pop_modal();
    controller.redraw().unwrap();

    let editor_layout = controller.current_layout();
    let (max_row, cursor_row, last_col) = {
        let ops = operations.borrow();
        let clear_count = ops.iter().filter(|op| **op == Operation::Clear).count();
        assert!(clear_count >= modal_layout.height());

        let mut cursor_row = modal_cursor_row as isize;
        let mut max_row = cursor_row;
        let mut last_col = 0;
        for op in ops.iter() {
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
    };

    assert!(max_row < 15);
    assert_eq!(cursor_row, editor_layout.cursor_row() as isize);
    assert_eq!(last_col, editor_layout.cursor.column);

    operations.borrow_mut().clear();
    controller.state_mut().editor_mut().insert('x');
    controller.redraw().unwrap();
    assert_eq!(
        controller.current_layout().cursor.column,
        editor_layout.cursor.column + 1
    );
}

#[test]
fn searchable_modal_dismissal_synchronizes_cursor() {
    let (backend, operations, _width, _height) = FakeTerminal::with_size(80, 15);
    let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

    let body = (1..=20)
        .map(|i| format!("option description {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let modal = ModalState::new("Select Option", &body, vec![ModalOption::from("Item A")]).with_search(true);
    controller.state_mut().push_modal(modal);
    controller.redraw().unwrap();

    let modal_layout = controller.current_layout();
    assert!(modal_layout.height() <= 15);
    let modal_cursor_row = modal_layout.cursor_row();

    operations.borrow_mut().clear();
    controller.state_mut().pop_modal();
    controller.redraw().unwrap();

    let editor_layout = controller.current_layout();
    let (cursor_row, last_col) = {
        let ops = operations.borrow();
        let mut cursor_row = modal_cursor_row as isize;
        let mut last_col = 0;
        for op in ops.iter() {
            match op {
                Operation::Up(n) => cursor_row -= *n as isize,
                Operation::Down(n) => cursor_row += *n as isize,
                Operation::Column(c) => last_col = *c,
                Operation::Write(text) => cursor_row += text.matches("\r\n").count() as isize,
                _ => {}
            }
        }
        (cursor_row, last_col)
    };

    assert_eq!(cursor_row, editor_layout.cursor_row() as isize);
    assert_eq!(last_col, editor_layout.cursor.column);
}
