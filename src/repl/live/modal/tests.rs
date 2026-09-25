use super::*;
use crate::ui::interactive::{EditorState, InteractiveState, ModalOption, ModalState};

#[test]
fn test_apply_input_edit() {
    let mut state = EditorState::default();
    apply_input_edit(&mut state, UiAction::Insert('h'));
    apply_input_edit(&mut state, UiAction::Insert('i'));
    assert_eq!(state.text(), "hi");
    apply_input_edit(&mut state, UiAction::MoveLeft);
    apply_input_edit(&mut state, UiAction::Insert('o'));
    assert_eq!(state.text(), "hoi");
    apply_input_edit(&mut state, UiAction::Delete);
    assert_eq!(state.text(), "ho");
    apply_input_edit(&mut state, UiAction::Backspace);
    assert_eq!(state.text(), "h");
    apply_input_edit(&mut state, UiAction::Paste("ello world".to_string()));
    assert_eq!(state.text(), "hello world");

    apply_input_edit(&mut state, UiAction::MoveWordLeft);
    apply_input_edit(&mut state, UiAction::MoveWordRight);
    apply_input_edit(&mut state, UiAction::MoveToStart);
    apply_input_edit(&mut state, UiAction::MoveRight);
    apply_input_edit(&mut state, UiAction::MoveToEnd);
    apply_input_edit(&mut state, UiAction::InsertNewline);
    apply_input_edit(&mut state, UiAction::DeleteWordBackward);
    apply_input_edit(&mut state, UiAction::MoveToStart);
    apply_input_edit(&mut state, UiAction::DeleteWordForward);
    apply_input_edit(&mut state, UiAction::DeleteToLineEnd);
    apply_input_edit(&mut state, UiAction::DeleteToLineStart);
    apply_input_edit(&mut state, UiAction::Yank);
    apply_input_edit(&mut state, UiAction::Undo);
    apply_input_edit(&mut state, UiAction::Exit);
}

struct MockBackend;
impl TerminalBackend for MockBackend {
    fn set_raw_mode(&mut self, _: bool) -> std::io::Result<()> {
        Ok(())
    }
    fn size(&self) -> std::io::Result<(u16, u16)> {
        Ok((80, 24))
    }
    fn hide_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn show_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn move_up(&mut self, _: usize) -> std::io::Result<()> {
        Ok(())
    }
    fn move_down(&mut self, _: usize) -> std::io::Result<()> {
        Ok(())
    }
    fn move_to_column(&mut self, _: usize) -> std::io::Result<()> {
        Ok(())
    }
    fn clear_line(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn write_text(&mut self, _: &str) -> std::io::Result<()> {
        Ok(())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn test_handle_modal_paste_input_mode() {
    let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
    assert!(!handle_modal_paste(&mut controller, "text"));

    let mut modal = ModalState::new("Permission Required", "", vec![]);
    modal.enter_input_mode("edit");
    controller.state_mut().push_modal(modal);

    assert!(handle_modal_paste(&mut controller, "cargo check"));
    assert_eq!(controller.state().active_modal().unwrap().input.text(), "cargo check");
}

#[test]
fn test_handle_modal_paste_select_mode_with_input_option() {
    let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
    let options = vec![
        ModalOption {
            label: "Allow".into(),
            description: None,
            input: None,
        },
        ModalOption {
            label: "Edit".into(),
            description: None,
            input: Some(crate::ui::interactive::InteractionInput {
                label: "edit".into(),
                value: Some("cargo test".into()),
            }),
        },
    ];
    let mut modal = ModalState::new("Permission Required", "", options);
    modal.selected = 1;
    controller.state_mut().push_modal(modal);

    assert!(handle_modal_paste(&mut controller, " --lib"));
    let active = controller.state().active_modal().unwrap();
    assert!(matches!(active.mode, ModalMode::Input { .. }));
    assert_eq!(active.input.text(), "cargo test --lib");
}

#[test]
fn test_handle_modal_paste_searchable_mode() {
    let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
    let modal = ModalState::new("Select Model", "", vec![]).with_search(true);
    controller.state_mut().push_modal(modal);

    assert!(handle_modal_paste(&mut controller, "claude"));
    let active = controller.state().active_modal().unwrap();
    assert_eq!(active.filter_query, "claude");
}

#[test]
fn test_handle_selector_nav_movement() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
    let options = vec![
        ModalOption {
            label: "Opt 1".into(),
            description: None,
            input: None,
        },
        ModalOption {
            label: "Opt 2".into(),
            description: None,
            input: None,
        },
        ModalOption {
            label: "Opt 3".into(),
            description: None,
            input: None,
        },
    ];
    let modal = ModalState::new("Test Modal", "", options);
    controller.state_mut().push_modal(modal);

    let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    assert!(handle_selector_nav(&mut controller, &down).unwrap());
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    assert!(handle_selector_nav(&mut controller, &tab).unwrap());
    assert_eq!(controller.state().active_modal().unwrap().selected, 2);

    let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
    assert!(handle_selector_nav(&mut controller, &up).unwrap());
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let backtab = KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE);
    assert!(handle_selector_nav(&mut controller, &backtab).unwrap());
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    let shift_tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT);
    assert!(handle_selector_nav(&mut controller, &shift_tab).unwrap());
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);

    let key_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
    assert!(handle_selector_nav(&mut controller, &key_j).unwrap());
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let key_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
    assert!(handle_selector_nav(&mut controller, &key_k).unwrap());
    assert_eq!(controller.state().active_modal().unwrap().selected, 0);
}

#[test]
fn test_handle_selector_nav_digits_and_unhandled() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
    let options = vec![
        ModalOption {
            label: "Opt 1".into(),
            description: None,
            input: None,
        },
        ModalOption {
            label: "Opt 2".into(),
            description: None,
            input: None,
        },
    ];
    let modal = ModalState::new("Test Modal", "", options);
    controller.state_mut().push_modal(modal);

    let key_2 = KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE);
    assert!(handle_selector_nav(&mut controller, &key_2).unwrap());
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let key_9 = KeyEvent::new(KeyCode::Char('9'), KeyModifiers::NONE);
    assert!(handle_selector_nav(&mut controller, &key_9).unwrap());
    assert_eq!(controller.state().active_modal().unwrap().selected, 1);

    let ctrl_2 = KeyEvent::new(KeyCode::Char('2'), KeyModifiers::CONTROL);
    assert!(!handle_selector_nav(&mut controller, &ctrl_2).unwrap());

    let unhandled = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    assert!(!handle_selector_nav(&mut controller, &unhandled).unwrap());
}

#[test]
fn test_handle_selector_nav_searchable_and_filter() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut controller = TerminalController::new(MockBackend, InteractiveState::default()).unwrap();
    let modal = ModalState::new("Search Modal", "", vec![]).with_search(true);
    controller.state_mut().push_modal(modal);

    let key_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
    assert!(handle_selector_nav(&mut controller, &key_a).unwrap());
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "a");

    let key_b = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE);
    assert!(handle_selector_nav(&mut controller, &key_b).unwrap());
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "ab");

    let backspace = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
    assert!(handle_selector_nav(&mut controller, &backspace).unwrap());
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "a");

    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(handle_selector_nav(&mut controller, &ctrl_c).unwrap());
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "");

    assert!(handle_selector_nav(&mut controller, &ctrl_c).unwrap());
    assert!(controller.state().active_modal().is_none());
}
