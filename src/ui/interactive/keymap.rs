use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyAction {
    AppInterrupt,
    AppClear,
    AppExit,
    AppSuspend,
    AppEditorExternal,
    AppClipboardPasteImage,
    AppModelSelect,
    AppModelCycleForward,
    AppModelCycleBackward,
    AppThinkingCycle,
    AppThinkingToggle,
    AppToolsExpand,
    AppMessageCopy,
    AppMessageFollowUp,
    AppMessageDequeue,
    AppSessionNew,
    AppSessionTree,
    AppSessionFork,
    AppSessionResume,
    TuiEditorCursorUp,
    TuiEditorCursorDown,
    TuiEditorCursorLeft,
    TuiEditorCursorRight,
    TuiEditorCursorWordLeft,
    TuiEditorCursorWordRight,
    TuiEditorCursorLineStart,
    TuiEditorCursorLineEnd,
    TuiEditorDeleteCharBackward,
    TuiEditorDeleteCharForward,
    TuiEditorDeleteWordBackward,
    TuiEditorDeleteWordForward,
    TuiEditorDeleteToLineStart,
    TuiEditorDeleteToLineEnd,
    TuiEditorYank,
    TuiEditorUndo,
    TuiInputNewLine,
    TuiInputSubmit,
    TuiInputTab,
    TuiSelectUp,
    TuiSelectDown,
    TuiSelectConfirm,
    TuiSelectCancel,
}

impl KeyAction {
    pub fn id(&self) -> &'static str {
        match self {
            Self::AppInterrupt => "app.interrupt",
            Self::AppClear => "app.clear",
            Self::AppExit => "app.exit",
            Self::AppSuspend => "app.suspend",
            Self::AppEditorExternal => "app.editor.external",
            Self::AppClipboardPasteImage => "app.clipboard.pasteImage",
            Self::AppModelSelect => "app.model.select",
            Self::AppModelCycleForward => "app.model.cycleForward",
            Self::AppModelCycleBackward => "app.model.cycleBackward",
            Self::AppThinkingCycle => "app.thinking.cycle",
            Self::AppThinkingToggle => "app.thinking.toggle",
            Self::AppToolsExpand => "app.tools.expand",
            Self::AppMessageCopy => "app.message.copy",
            Self::AppMessageFollowUp => "app.message.followUp",
            Self::AppMessageDequeue => "app.message.dequeue",
            Self::AppSessionNew => "app.session.new",
            Self::AppSessionTree => "app.session.tree",
            Self::AppSessionFork => "app.session.fork",
            Self::AppSessionResume => "app.session.resume",
            Self::TuiEditorCursorUp => "tui.editor.cursorUp",
            Self::TuiEditorCursorDown => "tui.editor.cursorDown",
            Self::TuiEditorCursorLeft => "tui.editor.cursorLeft",
            Self::TuiEditorCursorRight => "tui.editor.cursorRight",
            Self::TuiEditorCursorWordLeft => "tui.editor.cursorWordLeft",
            Self::TuiEditorCursorWordRight => "tui.editor.cursorWordRight",
            Self::TuiEditorCursorLineStart => "tui.editor.cursorLineStart",
            Self::TuiEditorCursorLineEnd => "tui.editor.cursorLineEnd",
            Self::TuiEditorDeleteCharBackward => "tui.editor.deleteCharBackward",
            Self::TuiEditorDeleteCharForward => "tui.editor.deleteCharForward",
            Self::TuiEditorDeleteWordBackward => "tui.editor.deleteWordBackward",
            Self::TuiEditorDeleteWordForward => "tui.editor.deleteWordForward",
            Self::TuiEditorDeleteToLineStart => "tui.editor.deleteToLineStart",
            Self::TuiEditorDeleteToLineEnd => "tui.editor.deleteToLineEnd",
            Self::TuiEditorYank => "tui.editor.yank",
            Self::TuiEditorUndo => "tui.editor.undo",
            Self::TuiInputNewLine => "tui.input.newLine",
            Self::TuiInputSubmit => "tui.input.submit",
            Self::TuiInputTab => "tui.input.tab",
            Self::TuiSelectUp => "tui.select.up",
            Self::TuiSelectDown => "tui.select.down",
            Self::TuiSelectConfirm => "tui.select.confirm",
            Self::TuiSelectCancel => "tui.select.cancel",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "app.interrupt" => Some(Self::AppInterrupt),
            "app.clear" => Some(Self::AppClear),
            "app.exit" => Some(Self::AppExit),
            "app.suspend" => Some(Self::AppSuspend),
            "app.editor.external" => Some(Self::AppEditorExternal),
            "app.clipboard.pasteImage" => Some(Self::AppClipboardPasteImage),
            "app.model.select" => Some(Self::AppModelSelect),
            "app.model.cycleForward" => Some(Self::AppModelCycleForward),
            "app.model.cycleBackward" => Some(Self::AppModelCycleBackward),
            "app.thinking.cycle" => Some(Self::AppThinkingCycle),
            "app.thinking.toggle" => Some(Self::AppThinkingToggle),
            "app.tools.expand" => Some(Self::AppToolsExpand),
            "app.message.copy" => Some(Self::AppMessageCopy),
            "app.message.followUp" => Some(Self::AppMessageFollowUp),
            "app.message.dequeue" => Some(Self::AppMessageDequeue),
            "app.session.new" => Some(Self::AppSessionNew),
            "app.session.tree" => Some(Self::AppSessionTree),
            "app.session.fork" => Some(Self::AppSessionFork),
            "app.session.resume" => Some(Self::AppSessionResume),
            "tui.editor.cursorUp" => Some(Self::TuiEditorCursorUp),
            "tui.editor.cursorDown" => Some(Self::TuiEditorCursorDown),
            "tui.editor.cursorLeft" => Some(Self::TuiEditorCursorLeft),
            "tui.editor.cursorRight" => Some(Self::TuiEditorCursorRight),
            "tui.editor.cursorWordLeft" => Some(Self::TuiEditorCursorWordLeft),
            "tui.editor.cursorWordRight" => Some(Self::TuiEditorCursorWordRight),
            "tui.editor.cursorLineStart" => Some(Self::TuiEditorCursorLineStart),
            "tui.editor.cursorLineEnd" => Some(Self::TuiEditorCursorLineEnd),
            "tui.editor.deleteCharBackward" => Some(Self::TuiEditorDeleteCharBackward),
            "tui.editor.deleteCharForward" => Some(Self::TuiEditorDeleteCharForward),
            "tui.editor.deleteWordBackward" => Some(Self::TuiEditorDeleteWordBackward),
            "tui.editor.deleteWordForward" => Some(Self::TuiEditorDeleteWordForward),
            "tui.editor.deleteToLineStart" => Some(Self::TuiEditorDeleteToLineStart),
            "tui.editor.deleteToLineEnd" => Some(Self::TuiEditorDeleteToLineEnd),
            "tui.editor.yank" => Some(Self::TuiEditorYank),
            "tui.editor.undo" => Some(Self::TuiEditorUndo),
            "tui.input.newLine" => Some(Self::TuiInputNewLine),
            "tui.input.submit" => Some(Self::TuiInputSubmit),
            "tui.input.tab" => Some(Self::TuiInputTab),
            "tui.select.up" => Some(Self::TuiSelectUp),
            "tui.select.down" => Some(Self::TuiSelectDown),
            "tui.select.confirm" => Some(Self::TuiSelectConfirm),
            "tui.select.cancel" => Some(Self::TuiSelectCancel),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyChord {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    pub fn matches(&self, event: &KeyEvent) -> bool {
        if event.kind == KeyEventKind::Release {
            return false;
        }
        let norm_event_code = match event.code {
            KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
            other => other,
        };
        let norm_self_code = match self.code {
            KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
            other => other,
        };
        norm_event_code == norm_self_code && event.modifiers == self.modifiers
    }
}

#[derive(Debug, Clone, Default)]
pub struct KeybindingMap {
    bindings: HashMap<KeyChord, KeyAction>,
    action_keys: HashMap<KeyAction, Vec<KeyChord>>,
}

impl KeybindingMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind(&mut self, chord: KeyChord, action: KeyAction) {
        self.bindings.insert(chord, action);
        self.action_keys.entry(action).or_default().push(chord);
    }

    pub fn unbind_action(&mut self, action: KeyAction) {
        if let Some(chords) = self.action_keys.remove(&action) {
            for chord in chords {
                self.bindings.remove(&chord);
            }
        }
    }

    pub fn get_action(&self, event: &KeyEvent) -> Option<KeyAction> {
        let norm_code = match event.code {
            KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
            other => other,
        };
        let chord = KeyChord::new(norm_code, event.modifiers);
        self.bindings.get(&chord).copied()
    }
}
