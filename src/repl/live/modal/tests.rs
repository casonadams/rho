use super::interaction::{is_input_trigger, prompt_label_for};
use super::*;
use crate::ui::interactive::EditorState;

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
}
