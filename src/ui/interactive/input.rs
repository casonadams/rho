use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::keymap::{KeyAction, KeybindingMap};
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
    SessionNew,
    SessionTree,
    SessionResume,
    HistoryPrevious,
    HistoryNext,
    Complete,
    Edit(UiAction),
    Ignore,
}

pub fn map_key(event: KeyEvent) -> InputAction {
    let bindings = super::keymap::default_keybindings();
    map_key_with_bindings(event, &bindings)
}

fn map_app_action(action: KeyAction) -> Option<InputAction> {
    Some(match action {
        KeyAction::AppInterrupt => InputAction::Cancel,
        KeyAction::AppClear => InputAction::Clear,
        KeyAction::AppExit => InputAction::EndOfInput,
        KeyAction::AppSuspend => InputAction::Suspend,
        KeyAction::AppEditorExternal => InputAction::ExternalEditor,
        KeyAction::AppClipboardPasteImage => InputAction::ClipboardPasteImage,
        KeyAction::AppModelSelect => InputAction::ModelSelect,
        KeyAction::AppModelCycleForward => InputAction::ModelCycleForward,
        KeyAction::AppModelCycleBackward => InputAction::ModelCycleBackward,
        KeyAction::AppThinkingCycle => InputAction::ThinkingCycle,
        KeyAction::AppThinkingToggle => InputAction::ThinkingToggle,
        KeyAction::AppToolsExpand => InputAction::ToggleExpandTools,
        KeyAction::AppMessageCopy => InputAction::MessageCopy,
        KeyAction::AppMessageFollowUp => InputAction::Edit(UiAction::Submit(QueueKind::FollowUp)),
        KeyAction::AppMessageDequeue => InputAction::DequeueQueued,
        KeyAction::AppSessionNew => InputAction::SessionNew,
        KeyAction::AppSessionTree => InputAction::SessionTree,
        KeyAction::AppSessionResume => InputAction::SessionResume,
        KeyAction::AppSessionFork => InputAction::Ignore,
        _ => return None,
    })
}

fn map_tui_cursor_action(action: KeyAction) -> Option<InputAction> {
    Some(match action {
        KeyAction::TuiEditorCursorUp | KeyAction::TuiSelectUp => InputAction::HistoryPrevious,
        KeyAction::TuiEditorCursorDown | KeyAction::TuiSelectDown => InputAction::HistoryNext,
        KeyAction::TuiEditorCursorLeft => InputAction::Edit(UiAction::MoveLeft),
        KeyAction::TuiEditorCursorRight => InputAction::Edit(UiAction::MoveRight),
        KeyAction::TuiEditorCursorWordLeft => InputAction::Edit(UiAction::MoveWordLeft),
        KeyAction::TuiEditorCursorWordRight => InputAction::Edit(UiAction::MoveWordRight),
        KeyAction::TuiEditorCursorLineStart => InputAction::Edit(UiAction::MoveToStart),
        KeyAction::TuiEditorCursorLineEnd => InputAction::Edit(UiAction::MoveToEnd),
        _ => return None,
    })
}

fn map_tui_edit_action(action: KeyAction) -> Option<InputAction> {
    Some(match action {
        KeyAction::TuiEditorDeleteCharBackward => InputAction::Edit(UiAction::Backspace),
        KeyAction::TuiEditorDeleteCharForward => InputAction::Edit(UiAction::Delete),
        KeyAction::TuiEditorDeleteWordBackward => InputAction::Edit(UiAction::DeleteWordBackward),
        KeyAction::TuiEditorDeleteWordForward => InputAction::Edit(UiAction::DeleteWordForward),
        KeyAction::TuiEditorDeleteToLineStart => InputAction::Edit(UiAction::DeleteToLineStart),
        KeyAction::TuiEditorDeleteToLineEnd => InputAction::Edit(UiAction::DeleteToLineEnd),
        KeyAction::TuiEditorYank => InputAction::Edit(UiAction::Yank),
        KeyAction::TuiEditorUndo => InputAction::Edit(UiAction::Undo),
        KeyAction::TuiInputNewLine => InputAction::Edit(UiAction::InsertNewline),
        KeyAction::TuiInputSubmit | KeyAction::TuiSelectConfirm => {
            InputAction::Edit(UiAction::Submit(QueueKind::Steering))
        }
        KeyAction::TuiInputTab => InputAction::Complete,
        KeyAction::TuiSelectCancel => InputAction::Cancel,
        _ => return None,
    })
}

fn map_bound_action(action: KeyAction) -> InputAction {
    map_app_action(action)
        .or_else(|| map_tui_cursor_action(action))
        .or_else(|| map_tui_edit_action(action))
        .unwrap_or(InputAction::Ignore)
}

pub fn map_key_with_bindings(event: KeyEvent, bindings: &KeybindingMap) -> InputAction {
    if event.kind == KeyEventKind::Release {
        return InputAction::Ignore;
    }

    if event.code == KeyCode::Char('\n') {
        return InputAction::Edit(UiAction::InsertNewline);
    }

    if let Some(action) = bindings.get_action(&event) {
        return map_bound_action(action);
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
    use crate::ui::interactive::{QueueKind, UiAction};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
    fn raw_newline_char_inserts_a_newline() {
        assert_eq!(
            map_key(key(KeyCode::Char('\n'), KeyModifiers::NONE)),
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
            (key(KeyCode::Char('c'), KeyModifiers::CONTROL), InputAction::Clear),
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
}
