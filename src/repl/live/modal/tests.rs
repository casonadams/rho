use super::interaction::{is_input_trigger, prompt_label_for};
use super::session::format_relative_time;
use super::*;
use crate::ui::interactive::EditorState;
use chrono::{Duration, Utc};

#[test]
fn test_format_relative_time_intervals() {
    let now = Utc::now();
    assert_eq!(format_relative_time(now), "just now");
    assert_eq!(format_relative_time(now - Duration::seconds(30)), "just now");
    assert_eq!(format_relative_time(now - Duration::minutes(5)), "5m ago");
    assert_eq!(format_relative_time(now - Duration::hours(3)), "3h ago");
    assert_eq!(format_relative_time(now - Duration::days(4)), "4d ago");
    let old = now - Duration::days(40);
    assert_eq!(format_relative_time(old), old.format("%Y-%m-%d").to_string());
}

#[test]
fn test_is_input_trigger_and_prompt_labels() {
    assert!(is_input_trigger("Deny with reason"));
    assert!(is_input_trigger("Allow with feedback"));
    assert!(is_input_trigger("Type something"));
    assert!(is_input_trigger("Accept input"));
    assert!(!is_input_trigger("Yes, approve"));

    assert_eq!(prompt_label_for("Deny with reason"), "reason");
    assert_eq!(prompt_label_for("Permission requested"), "reason");
    assert_eq!(prompt_label_for("Approve tool"), "reason");
    assert_eq!(prompt_label_for("Type something"), "answer");
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
