use super::selectors::test_support::DummyBackend;
use super::*;
use crate::ui::interactive::{EditorState, InteractiveState, ModalOption, ModalState};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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

fn paste_into_modal(modal: Option<ModalState>, text: &str) -> (bool, TerminalController<DummyBackend>) {
    let mut controller = TerminalController::new(DummyBackend, InteractiveState::default()).unwrap();
    if let Some(modal) = modal {
        controller.state_mut().push_modal(modal);
    }
    let handled = handle_modal_paste(&mut controller, text);
    (handled, controller)
}

#[test]
fn test_handle_modal_paste_input_and_searchable_modes() {
    let (handled, _) = paste_into_modal(None, "text");
    assert!(!handled);

    let mut input_modal = ModalState::new("Permission Required", "", vec![]);
    input_modal.enter_input_mode("edit");
    let (handled, controller) = paste_into_modal(Some(input_modal), "cargo check");
    assert!(handled);
    assert_eq!(controller.state().active_modal().unwrap().input.text(), "cargo check");

    let search_modal = ModalState::new("Select Model", "", vec![]).with_search(true);
    let (handled, controller) = paste_into_modal(Some(search_modal), "claude");
    assert!(handled);
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "claude");
}

#[test]
fn test_handle_modal_paste_select_mode_with_input_option() {
    let mut controller = TerminalController::new(DummyBackend, InteractiveState::default()).unwrap();
    let options = vec![
        ModalOption {
            label: "Allow".into(),
            description: None,
            input: None,
            value: None,
        },
        ModalOption {
            label: "Edit".into(),
            description: None,
            input: Some(crate::ui::interactive::InteractionInput {
                label: "edit".into(),
                value: Some("cargo test".into()),
            }),
            value: None,
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

fn selector_with_options(labels: &[&str]) -> TerminalController<DummyBackend> {
    let mut controller = TerminalController::new(DummyBackend, InteractiveState::default()).unwrap();
    let options = labels
        .iter()
        .map(|label| ModalOption {
            label: (*label).into(),
            description: None,
            input: None,
            value: None,
        })
        .collect();
    controller
        .state_mut()
        .push_modal(ModalState::new("Test Modal", "", options));
    controller
}

fn press_nav(controller: &mut TerminalController<DummyBackend>, code: KeyCode, modifiers: KeyModifiers) -> bool {
    handle_selector_nav(controller, &KeyEvent::new(code, modifiers)).unwrap()
}

#[test]
fn test_handle_selector_nav_movement() {
    let mut controller = selector_with_options(&["Opt 1", "Opt 2", "Opt 3"]);
    let steps = [
        (KeyCode::Down, KeyModifiers::NONE, 1),
        (KeyCode::Tab, KeyModifiers::NONE, 2),
        (KeyCode::Up, KeyModifiers::NONE, 1),
        (KeyCode::BackTab, KeyModifiers::NONE, 0),
        (KeyCode::Tab, KeyModifiers::SHIFT, 0),
        (KeyCode::Char('j'), KeyModifiers::NONE, 1),
        (KeyCode::Char('k'), KeyModifiers::NONE, 0),
    ];
    for (code, modifiers, expected) in steps {
        assert!(press_nav(&mut controller, code, modifiers));
        assert_eq!(controller.state().active_modal().unwrap().selected, expected);
    }
}

#[test]
fn test_handle_selector_nav_digits_and_unhandled() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut controller = selector_with_options(&["Opt 1", "Opt 2"]);

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
    let mut controller = TerminalController::new(DummyBackend, InteractiveState::default()).unwrap();
    let modal = ModalState::new("Search Modal", "", vec![]).with_search(true);
    controller.state_mut().push_modal(modal);

    let steps = [
        (KeyCode::Char('a'), KeyModifiers::NONE, "a"),
        (KeyCode::Char('b'), KeyModifiers::NONE, "ab"),
        (KeyCode::Backspace, KeyModifiers::NONE, "a"),
        (KeyCode::Char('c'), KeyModifiers::CONTROL, ""),
    ];
    for (code, modifiers, expected) in steps {
        assert!(press_nav(&mut controller, code, modifiers));
        assert_eq!(controller.state().active_modal().unwrap().filter_query, expected);
    }

    assert!(press_nav(&mut controller, KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(controller.state().active_modal().is_none());
}
