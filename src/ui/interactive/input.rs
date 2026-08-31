use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{QueueKind, UiAction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    Edit(UiAction),
    HistoryPrevious,
    HistoryNext,
    Complete,
    Cancel,
    EndOfInput,
    ToggleExpandTools,
    DequeueQueued,
    ExternalEditor,
    Ignore,
}

pub fn map_key(event: KeyEvent) -> InputAction {
    if event.kind == KeyEventKind::Release {
        return InputAction::Ignore;
    }

    match (event.code, event.modifiers) {
        (KeyCode::Up, modifiers) if modifiers.contains(KeyModifiers::ALT) => InputAction::DequeueQueued,
        (KeyCode::Enter, modifiers) if modifiers.contains(KeyModifiers::ALT) => {
            InputAction::Edit(UiAction::Submit(QueueKind::FollowUp))
        }
        (KeyCode::Enter, modifiers) if modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL) => {
            InputAction::Edit(UiAction::InsertNewline)
        }
        (KeyCode::Enter, _) => InputAction::Edit(UiAction::Submit(QueueKind::Steering)),
        (KeyCode::Char('j' | 'J'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            InputAction::Edit(UiAction::InsertNewline)
        }
        (KeyCode::Char('o' | 'O'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            InputAction::ToggleExpandTools
        }
        (KeyCode::Char('g' | 'G'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            InputAction::ExternalEditor
        }
        (KeyCode::Char('d' | 'D'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => InputAction::EndOfInput,
        (KeyCode::Char('c' | 'C'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => InputAction::Cancel,
        (KeyCode::Char(character), modifiers) if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            InputAction::Edit(UiAction::Insert(character))
        }
        (KeyCode::Backspace, _) => InputAction::Edit(UiAction::Backspace),
        (KeyCode::Delete, _) => InputAction::Edit(UiAction::Delete),
        (KeyCode::Left, _) => InputAction::Edit(UiAction::MoveLeft),
        (KeyCode::Right, _) => InputAction::Edit(UiAction::MoveRight),
        (KeyCode::Home, _) => InputAction::Edit(UiAction::MoveToStart),
        (KeyCode::End, _) => InputAction::Edit(UiAction::MoveToEnd),
        (KeyCode::Up, _) => InputAction::HistoryPrevious,
        (KeyCode::Down, _) => InputAction::HistoryNext,
        (KeyCode::Tab, _) => InputAction::Complete,
        (KeyCode::Esc, _) => InputAction::Cancel,
        _ => InputAction::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

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
            (
                key(KeyCode::Char('o'), KeyModifiers::CONTROL),
                InputAction::ToggleExpandTools,
            ),
        ];

        for (event, expected) in cases {
            assert_eq!(map_key(event), expected);
        }
    }

    #[test]
    fn release_and_unhandled_modified_keys_are_ignored() {
        let release = KeyEvent {
            code: KeyCode::Char('x'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        assert_eq!(map_key(release), InputAction::Ignore);
        assert_eq!(map_key(key(KeyCode::Char('x'), KeyModifiers::ALT)), InputAction::Ignore);
    }
}
