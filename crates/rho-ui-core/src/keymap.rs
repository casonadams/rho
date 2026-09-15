use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueueKind {
    Steering,
    FollowUp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiAction {
    Insert(char),
    InsertNewline,
    Backspace,
    Delete,
    MoveLeft,
    MoveRight,
    MoveWordLeft,
    MoveWordRight,
    MoveToStart,
    MoveToEnd,
    DeleteWordBackward,
    DeleteWordForward,
    DeleteToLineStart,
    DeleteToLineEnd,
    Yank,
    Undo,
    Paste(String),
    Submit(QueueKind),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

const ACTION_MAP: &[(KeyAction, &str)] = &[
    (KeyAction::AppInterrupt, "app.interrupt"),
    (KeyAction::AppClear, "app.clear"),
    (KeyAction::AppExit, "app.exit"),
    (KeyAction::AppSuspend, "app.suspend"),
    (KeyAction::AppEditorExternal, "app.editor.external"),
    (KeyAction::AppClipboardPasteImage, "app.clipboard.pasteImage"),
    (KeyAction::AppModelSelect, "app.model.select"),
    (KeyAction::AppModelCycleForward, "app.model.cycleForward"),
    (KeyAction::AppModelCycleBackward, "app.model.cycleBackward"),
    (KeyAction::AppThinkingCycle, "app.thinking.cycle"),
    (KeyAction::AppThinkingToggle, "app.thinking.toggle"),
    (KeyAction::AppToolsExpand, "app.tools.expand"),
    (KeyAction::AppMessageCopy, "app.message.copy"),
    (KeyAction::AppMessageFollowUp, "app.message.followUp"),
    (KeyAction::AppMessageDequeue, "app.message.dequeue"),
    (KeyAction::AppSessionNew, "app.session.new"),
    (KeyAction::AppSessionTree, "app.session.tree"),
    (KeyAction::AppSessionFork, "app.session.fork"),
    (KeyAction::AppSessionResume, "app.session.resume"),
    (KeyAction::TuiEditorCursorUp, "tui.editor.cursorUp"),
    (KeyAction::TuiEditorCursorDown, "tui.editor.cursorDown"),
    (KeyAction::TuiEditorCursorLeft, "tui.editor.cursorLeft"),
    (KeyAction::TuiEditorCursorRight, "tui.editor.cursorRight"),
    (KeyAction::TuiEditorCursorWordLeft, "tui.editor.cursorWordLeft"),
    (KeyAction::TuiEditorCursorWordRight, "tui.editor.cursorWordRight"),
    (KeyAction::TuiEditorCursorLineStart, "tui.editor.cursorLineStart"),
    (KeyAction::TuiEditorCursorLineEnd, "tui.editor.cursorLineEnd"),
    (KeyAction::TuiEditorDeleteCharBackward, "tui.editor.deleteCharBackward"),
    (KeyAction::TuiEditorDeleteCharForward, "tui.editor.deleteCharForward"),
    (KeyAction::TuiEditorDeleteWordBackward, "tui.editor.deleteWordBackward"),
    (KeyAction::TuiEditorDeleteWordForward, "tui.editor.deleteWordForward"),
    (KeyAction::TuiEditorDeleteToLineStart, "tui.editor.deleteToLineStart"),
    (KeyAction::TuiEditorDeleteToLineEnd, "tui.editor.deleteToLineEnd"),
    (KeyAction::TuiEditorYank, "tui.editor.yank"),
    (KeyAction::TuiEditorUndo, "tui.editor.undo"),
    (KeyAction::TuiInputNewLine, "tui.input.newLine"),
    (KeyAction::TuiInputSubmit, "tui.input.submit"),
    (KeyAction::TuiInputTab, "tui.input.tab"),
    (KeyAction::TuiSelectUp, "tui.select.up"),
    (KeyAction::TuiSelectDown, "tui.select.down"),
    (KeyAction::TuiSelectConfirm, "tui.select.confirm"),
    (KeyAction::TuiSelectCancel, "tui.select.cancel"),
];

impl KeyAction {
    pub fn as_str(self) -> &'static str {
        ACTION_MAP
            .iter()
            .find(|(action, _)| *action == self)
            .map(|(_, name)| *name)
            .unwrap_or("")
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::from_name(id)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        ACTION_MAP.iter().find(|(_, n)| *n == name).map(|(action, _)| *action)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::HashMap;

    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    use super::*;

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

            let is_self_shift_tab = (self.code == KeyCode::Tab && self.modifiers.contains(KeyModifiers::SHIFT))
                || self.code == KeyCode::BackTab;
            let is_event_shift_tab = (event.code == KeyCode::Tab && event.modifiers.contains(KeyModifiers::SHIFT))
                || event.code == KeyCode::BackTab;

            if is_self_shift_tab && is_event_shift_tab {
                return true;
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
            for (chord, action) in &self.bindings {
                if chord.matches(event) {
                    return Some(*action);
                }
            }
            None
        }
    }

    fn apply_modifier(modifiers: &mut KeyModifiers, lower: &str) {
        let flag = match lower {
            "ctrl" | "control" => KeyModifiers::CONTROL,
            "alt" | "opt" | "option" => KeyModifiers::ALT,
            "shift" => KeyModifiers::SHIFT,
            "super" | "cmd" | "command" => KeyModifiers::SUPER,
            _ => KeyModifiers::NONE,
        };
        *modifiers |= flag;
    }

    pub fn parse_key_code(raw: &str) -> Option<KeyCode> {
        match raw.to_ascii_lowercase().as_str() {
            "enter" | "return" => Some(KeyCode::Enter),
            "esc" | "escape" => Some(KeyCode::Esc),
            "backspace" => Some(KeyCode::Backspace),
            "tab" => Some(KeyCode::Tab),
            "backtab" => Some(KeyCode::BackTab),
            "delete" | "del" => Some(KeyCode::Delete),
            "insert" | "ins" => Some(KeyCode::Insert),
            "up" => Some(KeyCode::Up),
            "down" => Some(KeyCode::Down),
            "left" => Some(KeyCode::Left),
            "right" => Some(KeyCode::Right),
            "home" => Some(KeyCode::Home),
            "end" => Some(KeyCode::End),
            "pageup" => Some(KeyCode::PageUp),
            "pagedown" => Some(KeyCode::PageDown),
            "space" => Some(KeyCode::Char(' ')),
            s if s.starts_with('f') && s[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) => {
                Some(KeyCode::F(s[1..].parse::<u8>().unwrap()))
            }
            s if s.chars().count() == 1 => Some(KeyCode::Char(s.chars().next().unwrap())),
            _ => None,
        }
    }

    pub fn parse_key_chord(raw: &str) -> Option<KeyChord> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }

        let parts: Vec<&str> = raw.split('+').map(str::trim).collect();
        if parts.is_empty() {
            return None;
        }

        let mut modifiers = KeyModifiers::NONE;
        let mut key_part = "";

        for (i, part) in parts.iter().enumerate() {
            let lower = part.to_ascii_lowercase();
            if i == parts.len() - 1 {
                key_part = part;
                break;
            }
            apply_modifier(&mut modifiers, &lower);
        }

        let code = parse_key_code(key_part)?;
        Some(KeyChord::new(code, modifiers))
    }

    pub fn default_keybindings() -> KeybindingMap {
        const DEFAULT_KEYBINDING_DEFS: &[(&str, &[&str])] = &[
            ("app.interrupt", &["escape"]),
            ("app.clear", &["ctrl+c"]),
            ("app.exit", &["ctrl+d"]),
            ("app.suspend", &["ctrl+z"]),
            ("app.editor.external", &["ctrl+g"]),
            ("app.clipboard.pasteImage", &["ctrl+v"]),
            ("app.model.select", &["ctrl+l"]),
            ("app.model.cycleForward", &["ctrl+p"]),
            ("app.model.cycleBackward", &["shift+ctrl+p", "alt+p"]),
            ("app.thinking.cycle", &["shift+tab"]),
            ("app.thinking.toggle", &["ctrl+t"]),
            ("app.tools.expand", &["ctrl+o"]),
            ("app.message.copy", &["ctrl+x"]),
            ("app.message.followUp", &["alt+enter"]),
            ("app.message.dequeue", &["alt+up"]),
            ("tui.editor.cursorUp", &["up"]),
            ("tui.editor.cursorDown", &["down"]),
            ("tui.editor.cursorLeft", &["left", "ctrl+b"]),
            ("tui.editor.cursorRight", &["right", "ctrl+f"]),
            ("tui.editor.cursorWordLeft", &["alt+left", "ctrl+left", "alt+b"]),
            ("tui.editor.cursorWordRight", &["alt+right", "ctrl+right", "alt+f"]),
            ("tui.editor.cursorLineStart", &["home", "ctrl+home", "ctrl+a"]),
            ("tui.editor.cursorLineEnd", &["end", "ctrl+end", "ctrl+e"]),
            ("tui.editor.deleteCharBackward", &["backspace"]),
            ("tui.editor.deleteCharForward", &["delete"]),
            ("tui.editor.deleteWordBackward", &["ctrl+w", "alt+backspace"]),
            ("tui.editor.deleteWordForward", &["alt+d", "alt+delete"]),
            ("tui.editor.deleteToLineStart", &["ctrl+u"]),
            ("tui.editor.deleteToLineEnd", &["ctrl+k"]),
            ("tui.editor.yank", &["ctrl+y"]),
            ("tui.editor.undo", &["ctrl+-"]),
            ("tui.input.newLine", &["shift+enter", "ctrl+j", "ctrl+enter"]),
            ("tui.input.submit", &["enter"]),
            ("tui.input.tab", &["tab"]),
            ("tui.select.up", &["up"]),
            ("tui.select.down", &["down"]),
            ("tui.select.confirm", &["enter"]),
            ("tui.select.cancel", &["escape"]),
        ];

        let mut map = KeybindingMap::new();
        for (action_id, chords) in DEFAULT_KEYBINDING_DEFS {
            if let Some(action) = KeyAction::from_id(action_id) {
                for chord_str in *chords {
                    if let Some(chord) = parse_key_chord(chord_str) {
                        map.bind(chord, action);
                    }
                }
            }
        }
        map
    }

    pub fn map_app_action(action: KeyAction) -> Option<InputAction> {
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

    pub fn map_key(event: KeyEvent) -> InputAction {
        let bindings = default_keybindings();
        map_key_with_bindings(event, &bindings)
    }

    pub fn map_key_with_bindings(event: KeyEvent, bindings: &KeybindingMap) -> InputAction {
        if event.kind == KeyEventKind::Release {
            return InputAction::Ignore;
        }

        if let Some(action) = bindings.get_action(&event) {
            return map_bound_action(action);
        }

        match (event.code, event.modifiers) {
            (KeyCode::Char(c), mods) if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                InputAction::Edit(UiAction::Insert(c))
            }
            (KeyCode::Backspace, mods) if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                InputAction::Edit(UiAction::Backspace)
            }
            (KeyCode::Delete, mods) if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                InputAction::Edit(UiAction::Delete)
            }
            _ => InputAction::Ignore,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
