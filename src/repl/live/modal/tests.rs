use super::interaction::{is_input_trigger, prompt_label_for};
use super::*;
use crate::ui::interactive::{EditorState, InteractiveState, ModalOption, ModalState};

#[test]
fn test_is_input_trigger() {
    for trigger in [
        "Deny with reason",
        "Allow with feedback",
        "Type something",
        "Accept input",
    ] {
        assert!(is_input_trigger(trigger));
    }
    assert!(!is_input_trigger("Yes, approve"));
}

#[test]
fn test_prompt_labels_for_triggers() {
    let label_cases = [
        ("Deny with reason", "reason"),
        ("Permission requested", "reason"),
        ("Approve tool", "reason"),
        ("Type something", "answer"),
    ];
    for (prompt, expected) in label_cases {
        assert_eq!(prompt_label_for(prompt), expected);
    }
}

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
    apply_input_edit(&mut state, UiAction::Paste("ello".to_string()));
    assert_eq!(state.text(), "hello");
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
