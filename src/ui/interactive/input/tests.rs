use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{InputAction, map_key};
use crate::ui::interactive::{QueueKind, UiAction};

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn enter_variants_preserve_submission_intent() {
    let cases = [
        (
            key(KeyCode::Enter, KeyModifiers::NONE),
            InputAction::Edit(UiAction::Submit(QueueKind::Steering)),
        ),
        (
            key(KeyCode::Enter, KeyModifiers::ALT),
            InputAction::Edit(UiAction::Submit(QueueKind::FollowUp)),
        ),
        (
            key(KeyCode::Enter, KeyModifiers::SHIFT),
            InputAction::Edit(UiAction::InsertNewline),
        ),
        (
            key(KeyCode::Enter, KeyModifiers::CONTROL),
            InputAction::Edit(UiAction::InsertNewline),
        ),
    ];

    for (event, expected) in cases {
        assert_eq!(map_key(event), expected);
    }
}

#[test]
fn raw_ctrl_j_inserts_a_newline() {
    assert_eq!(
        map_key(key(KeyCode::Char('j'), KeyModifiers::CONTROL)),
        InputAction::Edit(UiAction::InsertNewline)
    );
}

#[test]
fn editor_navigation_and_control_keys_are_mapped() {
    let cases = [
        (
            key(KeyCode::Left, KeyModifiers::NONE),
            InputAction::Edit(UiAction::MoveLeft),
        ),
        (
            key(KeyCode::Right, KeyModifiers::NONE),
            InputAction::Edit(UiAction::MoveRight),
        ),
        (key(KeyCode::Up, KeyModifiers::ALT), InputAction::DequeueQueued),
        (key(KeyCode::Up, KeyModifiers::NONE), InputAction::HistoryPrevious),
        (key(KeyCode::Down, KeyModifiers::NONE), InputAction::HistoryNext),
        (key(KeyCode::Tab, KeyModifiers::NONE), InputAction::Complete),
        (key(KeyCode::Esc, KeyModifiers::NONE), InputAction::Cancel),
        (key(KeyCode::Char('d'), KeyModifiers::CONTROL), InputAction::EndOfInput),
        (key(KeyCode::Char('c'), KeyModifiers::CONTROL), InputAction::Cancel),
        (key(KeyCode::Char('l'), KeyModifiers::CONTROL), InputAction::ModelSelect),
        (
            key(KeyCode::Char('p'), KeyModifiers::CONTROL),
            InputAction::ModelCycleForward,
        ),
        (key(KeyCode::Tab, KeyModifiers::SHIFT), InputAction::ThinkingCycle),
        (
            key(KeyCode::Char('t'), KeyModifiers::CONTROL),
            InputAction::ThinkingToggle,
        ),
        (key(KeyCode::Char('x'), KeyModifiers::CONTROL), InputAction::MessageCopy),
        (
            key(KeyCode::Char('v'), KeyModifiers::CONTROL),
            InputAction::ClipboardPasteImage,
        ),
        (key(KeyCode::Char('z'), KeyModifiers::CONTROL), InputAction::Suspend),
        (
            key(KeyCode::Char('w'), KeyModifiers::CONTROL),
            InputAction::Edit(UiAction::DeleteWordBackward),
        ),
        (
            key(KeyCode::Char('k'), KeyModifiers::CONTROL),
            InputAction::Edit(UiAction::DeleteToLineEnd),
        ),
        (
            key(KeyCode::Char('y'), KeyModifiers::CONTROL),
            InputAction::Edit(UiAction::Yank),
        ),
        (
            key(KeyCode::Char('-'), KeyModifiers::CONTROL),
            InputAction::Edit(UiAction::Undo),
        ),
    ];

    for (event, expected) in cases {
        assert_eq!(map_key(event), expected);
    }
}
