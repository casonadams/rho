use crate::repl::live::modal::{ModalKeyResult, handle_modal_key, open_help_selector};
use crate::repl::live::tests::common::HistoryTerminal;
use crate::ui::interactive::{InteractiveState, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn send_modal_key(c: &mut TerminalController<HistoryTerminal>, code: KeyCode) -> ModalKeyResult {
    handle_modal_key(c, KeyEvent::new(code, KeyModifiers::NONE), &mut None).unwrap()
}

fn send_modal_char(c: &mut TerminalController<HistoryTerminal>, ch: char) -> ModalKeyResult {
    handle_modal_key(c, KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), &mut None).unwrap()
}

#[test]
fn help_modal_opens_with_commands_and_shortcuts() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_help_selector(&mut controller);
    let modal = controller.state().active_modal().unwrap();
    assert_eq!(modal.title, "Help");
    assert_eq!(modal.body, "");
    assert!(modal.is_searchable);

    let labels: Vec<&str> = modal.options.iter().map(|o| o.label.trim()).collect();
    let expected_items = [
        "/settings",
        "/model",
        "/resume",
        "/session",
        "/compact",
        "/tree",
        "/mcp",
        "/login",
        "/clear",
        "Tab",
        "Shift+Tab",
        "Ctrl+L",
        "Escape",
    ];
    for expected in expected_items {
        assert!(labels.contains(&expected));
    }
}

#[test]
fn help_modal_navigates_and_filters() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_help_selector(&mut controller);
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    let _ = send_modal_key(&mut controller, KeyCode::Down);
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let _ = send_modal_key(&mut controller, KeyCode::Up);
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    // Filter by typing 'mcp'
    send_modal_char(&mut controller, 'm');
    send_modal_char(&mut controller, 'c');
    send_modal_char(&mut controller, 'p');

    let modal = controller.state().active_modal().unwrap();
    assert!(!modal.options.is_empty());
    assert_eq!(modal.options[0].label.trim(), "/mcp");

    // Backspace removes filter char
    send_modal_key(&mut controller, KeyCode::Backspace);
    let modal_back = controller.state().active_modal().unwrap();
    assert!(modal_back.options.len() > 1);
}

#[test]
fn help_modal_enter_on_command_returns_command_selected() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_help_selector(&mut controller);

    // First option is /settings
    let res = send_modal_key(&mut controller, KeyCode::Enter);
    assert_eq!(
        res,
        ModalKeyResult::HelpCommandSelected {
            command: "/settings".to_string()
        }
    );
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn help_modal_esc_and_ctrl_c_dismiss() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    open_help_selector(&mut controller);
    assert!(controller.state().active_modal().is_some());

    let res = send_modal_key(&mut controller, KeyCode::Esc);
    assert_eq!(res, ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_none());

    // Ctrl+C with query clears query first
    open_help_selector(&mut controller);
    send_modal_char(&mut controller, 'x');
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "x");

    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let res = handle_modal_key(&mut controller, ctrl_c, &mut None).unwrap();
    assert_eq!(res, ModalKeyResult::Handled);
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "");
    assert!(controller.state().active_modal().is_some());

    // Ctrl+C with empty query dismisses modal
    let res = handle_modal_key(&mut controller, ctrl_c, &mut None).unwrap();
    assert_eq!(res, ModalKeyResult::Handled);
    assert!(controller.state().active_modal().is_none());
}
