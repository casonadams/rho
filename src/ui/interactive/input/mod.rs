use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
    QueueKind, UiAction,
    keymap::{KeyAction, KeybindingMap},
};

#[cfg(test)]
mod tests;

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
    ModelSelect,
    ModelCycleForward,
    ModelCycleBackward,
    ThinkingCycle,
    ThinkingToggle,
    MessageCopy,
    ClipboardPasteImage,
    SessionTree,
    SessionResume,
    SessionNew,
    Suspend,
    Ignore,
}

pub fn map_key(event: KeyEvent) -> InputAction {
    let bindings = super::keybinding_loader::default_keybindings();
    map_key_with_bindings(event, &bindings)
}

pub fn map_key_with_bindings(event: KeyEvent, bindings: &KeybindingMap) -> InputAction {
    if event.kind == KeyEventKind::Release {
        return InputAction::Ignore;
    }

    if let Some(action) = bindings.get_action(&event) {
        return match action {
            KeyAction::AppInterrupt | KeyAction::AppClear => InputAction::Cancel,
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
            KeyAction::TuiEditorCursorUp => InputAction::HistoryPrevious,
            KeyAction::TuiEditorCursorDown => InputAction::HistoryNext,
            KeyAction::TuiEditorCursorLeft => InputAction::Edit(UiAction::MoveLeft),
            KeyAction::TuiEditorCursorRight => InputAction::Edit(UiAction::MoveRight),
            KeyAction::TuiEditorCursorWordLeft => InputAction::Edit(UiAction::MoveWordLeft),
            KeyAction::TuiEditorCursorWordRight => InputAction::Edit(UiAction::MoveWordRight),
            KeyAction::TuiEditorCursorLineStart => InputAction::Edit(UiAction::MoveToStart),
            KeyAction::TuiEditorCursorLineEnd => InputAction::Edit(UiAction::MoveToEnd),
            KeyAction::TuiEditorDeleteCharBackward => InputAction::Edit(UiAction::Backspace),
            KeyAction::TuiEditorDeleteCharForward => InputAction::Edit(UiAction::Delete),
            KeyAction::TuiEditorDeleteWordBackward => InputAction::Edit(UiAction::DeleteWordBackward),
            KeyAction::TuiEditorDeleteWordForward => InputAction::Edit(UiAction::DeleteWordForward),
            KeyAction::TuiEditorDeleteToLineStart => InputAction::Edit(UiAction::DeleteToLineStart),
            KeyAction::TuiEditorDeleteToLineEnd => InputAction::Edit(UiAction::DeleteToLineEnd),
            KeyAction::TuiEditorYank => InputAction::Edit(UiAction::Yank),
            KeyAction::TuiEditorUndo => InputAction::Edit(UiAction::Undo),
            KeyAction::TuiInputNewLine => InputAction::Edit(UiAction::InsertNewline),
            KeyAction::TuiInputSubmit => InputAction::Edit(UiAction::Submit(QueueKind::Steering)),
            KeyAction::TuiInputTab => InputAction::Complete,
            KeyAction::TuiSelectUp => InputAction::HistoryPrevious,
            KeyAction::TuiSelectDown => InputAction::HistoryNext,
            KeyAction::TuiSelectConfirm => InputAction::Edit(UiAction::Submit(QueueKind::Steering)),
            KeyAction::TuiSelectCancel => InputAction::Cancel,
        };
    }

    match (event.code, event.modifiers) {
        (KeyCode::Char(character), modifiers) if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            InputAction::Edit(UiAction::Insert(character))
        }
        _ => InputAction::Ignore,
    }
}
