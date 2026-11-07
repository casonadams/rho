use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{QueueKind, UiAction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    Cancel,
    Clear,
    EndOfInput,
    Suspend,
    ExternalEditor,
    ClipboardPasteImage,
    ModelSelect,
    ModelCycleForward,
    ModelCycleBackward,
    ThinkingCycle,
    ThinkingToggle,
    ToggleExpandTools,
    MessageCopy,
    DequeueQueued,
    HistoryPrevious,
    HistoryNext,
    Complete,
    Edit(UiAction),
    Ignore,
}

const CTRL_SHIFT: KeyModifiers =
    KeyModifiers::from_bits_retain(KeyModifiers::CONTROL.bits() | KeyModifiers::SHIFT.bits());

static BINDINGS: &[(KeyCode, KeyModifiers, InputAction)] = &[
    (KeyCode::Esc, KeyModifiers::NONE, InputAction::Cancel),
    (KeyCode::Char('c'), KeyModifiers::CONTROL, InputAction::Clear),
    (KeyCode::Char('d'), KeyModifiers::CONTROL, InputAction::EndOfInput),
    (KeyCode::Char('z'), KeyModifiers::CONTROL, InputAction::Suspend),
    (KeyCode::Char('g'), KeyModifiers::CONTROL, InputAction::ExternalEditor),
    (
        KeyCode::Char('v'),
        KeyModifiers::CONTROL,
        InputAction::ClipboardPasteImage,
    ),
    (KeyCode::Char('l'), KeyModifiers::CONTROL, InputAction::ModelSelect),
    (
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
        InputAction::ModelCycleForward,
    ),
    (KeyCode::Char('p'), KeyModifiers::ALT, InputAction::ModelCycleBackward),
    (KeyCode::Char('p'), CTRL_SHIFT, InputAction::ModelCycleBackward),
    (KeyCode::Char('t'), KeyModifiers::CONTROL, InputAction::ThinkingToggle),
    (
        KeyCode::Char('o'),
        KeyModifiers::CONTROL,
        InputAction::ToggleExpandTools,
    ),
    (KeyCode::Char('x'), KeyModifiers::CONTROL, InputAction::MessageCopy),
    (
        KeyCode::Enter,
        KeyModifiers::ALT,
        InputAction::Edit(UiAction::Submit(QueueKind::FollowUp)),
    ),
    (KeyCode::Up, KeyModifiers::ALT, InputAction::DequeueQueued),
    (KeyCode::Up, KeyModifiers::NONE, InputAction::HistoryPrevious),
    (KeyCode::Down, KeyModifiers::NONE, InputAction::HistoryNext),
    (KeyCode::Left, KeyModifiers::NONE, InputAction::Edit(UiAction::MoveLeft)),
    (
        KeyCode::Char('b'),
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::MoveLeft),
    ),
    (
        KeyCode::Right,
        KeyModifiers::NONE,
        InputAction::Edit(UiAction::MoveRight),
    ),
    (
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::MoveRight),
    ),
    (
        KeyCode::Left,
        KeyModifiers::ALT,
        InputAction::Edit(UiAction::MoveWordLeft),
    ),
    (
        KeyCode::Left,
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::MoveWordLeft),
    ),
    (
        KeyCode::Char('b'),
        KeyModifiers::ALT,
        InputAction::Edit(UiAction::MoveWordLeft),
    ),
    (
        KeyCode::Right,
        KeyModifiers::ALT,
        InputAction::Edit(UiAction::MoveWordRight),
    ),
    (
        KeyCode::Right,
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::MoveWordRight),
    ),
    (
        KeyCode::Char('f'),
        KeyModifiers::ALT,
        InputAction::Edit(UiAction::MoveWordRight),
    ),
    (
        KeyCode::Home,
        KeyModifiers::NONE,
        InputAction::Edit(UiAction::MoveToStart),
    ),
    (
        KeyCode::Home,
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::MoveToStart),
    ),
    (
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::MoveToStart),
    ),
    (KeyCode::End, KeyModifiers::NONE, InputAction::Edit(UiAction::MoveToEnd)),
    (
        KeyCode::End,
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::MoveToEnd),
    ),
    (
        KeyCode::Char('e'),
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::MoveToEnd),
    ),
    (
        KeyCode::Backspace,
        KeyModifiers::NONE,
        InputAction::Edit(UiAction::Backspace),
    ),
    (KeyCode::Delete, KeyModifiers::NONE, InputAction::Edit(UiAction::Delete)),
    (
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::DeleteWordBackward),
    ),
    (
        KeyCode::Backspace,
        KeyModifiers::ALT,
        InputAction::Edit(UiAction::DeleteWordBackward),
    ),
    (
        KeyCode::Char('d'),
        KeyModifiers::ALT,
        InputAction::Edit(UiAction::DeleteWordForward),
    ),
    (
        KeyCode::Delete,
        KeyModifiers::ALT,
        InputAction::Edit(UiAction::DeleteWordForward),
    ),
    (
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::DeleteToLineStart),
    ),
    (
        KeyCode::Char('k'),
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::DeleteToLineEnd),
    ),
    (
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::Yank),
    ),
    (
        KeyCode::Char('-'),
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::Undo),
    ),
    (
        KeyCode::Enter,
        KeyModifiers::SHIFT,
        InputAction::Edit(UiAction::InsertNewline),
    ),
    (
        KeyCode::Enter,
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::InsertNewline),
    ),
    (
        KeyCode::Char('j'),
        KeyModifiers::CONTROL,
        InputAction::Edit(UiAction::InsertNewline),
    ),
    (
        KeyCode::Enter,
        KeyModifiers::NONE,
        InputAction::Edit(UiAction::Submit(QueueKind::Steering)),
    ),
    (KeyCode::Tab, KeyModifiers::NONE, InputAction::Complete),
];

fn is_shift_tab(event: &KeyEvent) -> bool {
    event.code == KeyCode::BackTab || (event.code == KeyCode::Tab && event.modifiers.contains(KeyModifiers::SHIFT))
}

fn bound_action(key: KeyCode, mods: KeyModifiers) -> Option<InputAction> {
    BINDINGS
        .iter()
        .find(|(code, modifiers, _)| *code == key && *modifiers == mods)
        .map(|(_, _, action)| action.clone())
}

pub fn map_key(event: KeyEvent) -> InputAction {
    if event.kind == KeyEventKind::Release {
        return InputAction::Ignore;
    }

    if event.code == KeyCode::Char('\n') {
        return InputAction::Edit(UiAction::InsertNewline);
    }

    if is_shift_tab(&event) {
        return InputAction::ThinkingCycle;
    }

    let key = match event.code {
        KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
        other => other,
    };
    if let Some(action) = bound_action(key, event.modifiers) {
        return action;
    }

    match (event.code, event.modifiers) {
        (KeyCode::Char(c), mods) if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            InputAction::Edit(UiAction::Insert(c))
        }
        _ => InputAction::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn every_bound_chord_maps_to_its_action() {
        for (code, modifiers, expected) in BINDINGS {
            assert_eq!(map_key(key(*code, *modifiers)), *expected);
        }
    }

    #[test]
    fn shift_tab_variants_cycle_thinking() {
        for event in [
            key(KeyCode::BackTab, KeyModifiers::NONE),
            key(KeyCode::Tab, KeyModifiers::SHIFT),
        ] {
            assert_eq!(map_key(event), InputAction::ThinkingCycle);
        }
    }

    #[test]
    fn raw_newline_char_inserts_a_newline() {
        assert_eq!(
            map_key(key(KeyCode::Char('\n'), KeyModifiers::NONE)),
            InputAction::Edit(UiAction::InsertNewline)
        );
    }
}
