use super::interaction::{is_input_trigger, prompt_label_for};
use super::session::format_relative_time;
use super::*;
use crate::ui::interactive::EditorState;
use chrono::{Duration, Utc};

#[test]
fn test_format_relative_time_intervals() {
    let now = Utc::now();
    let old = now - Duration::days(40);
    let cases = [
        (now, "just now".to_string()),
        (now - Duration::seconds(30), "just now".to_string()),
        (now - Duration::minutes(5), "5m ago".to_string()),
        (now - Duration::hours(3), "3h ago".to_string()),
        (now - Duration::days(4), "4d ago".to_string()),
        (old, old.format("%Y-%m-%d").to_string()),
    ];
    for (time, expected) in cases {
        assert_eq!(format_relative_time(time), expected);
    }
}

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
