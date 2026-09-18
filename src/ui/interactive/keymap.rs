use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::collections::HashMap;
use std::path::Path;

use super::key_parser::parse_key_chord;

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

        // Special handling for Shift+Tab: in crossterm, Shift+Tab can arrive either as
        // (KeyCode::BackTab, KeyModifiers::NONE / SHIFT) OR (KeyCode::Tab, KeyModifiers::SHIFT)
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

fn bind_default_keys(map: &mut KeybindingMap, id: &str, keys: &[&str]) {
    if let Some(action) = KeyAction::from_id(id) {
        for key_str in keys {
            if let Some(chord) = parse_key_chord(key_str) {
                map.bind(chord, action);
            }
        }
    }
}

pub fn default_keybindings() -> KeybindingMap {
    let mut map = KeybindingMap::new();
    for (id, keys) in DEFAULT_KEYBINDING_DEFS {
        bind_default_keys(&mut map, id, keys);
    }
    map
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum ConfigEntry {
    Single(String),
    Multiple(Vec<String>),
}

fn find_keybindings_content(config_dir: &Path) -> Option<(String, bool)> {
    let toml_path = config_dir.join("keybindings.toml");
    if toml_path.exists() {
        return std::fs::read_to_string(&toml_path).ok().map(|c| (c, true));
    }
    let json_path = config_dir.join("keybindings.json");
    if json_path.exists() {
        return std::fs::read_to_string(&json_path).ok().map(|c| (c, false));
    }
    dirs::home_dir()
        .map(|h| h.join(".pi/agent/keybindings.json"))
        .filter(|p| p.exists())
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|c| (c, false))
}

fn parse_config_entries(content: &str, is_toml: bool) -> HashMap<String, ConfigEntry> {
    if is_toml {
        toml::from_str(content).unwrap_or_default()
    } else {
        serde_json::from_str(content).unwrap_or_default()
    }
}

fn apply_config_entry(map: &mut KeybindingMap, action: KeyAction, entry: ConfigEntry) {
    map.unbind_action(action);
    let keys: Vec<String> = match entry {
        ConfigEntry::Single(k) => vec![k],
        ConfigEntry::Multiple(ks) => ks,
    };
    for k in &keys {
        if let Some(chord) = parse_key_chord(k) {
            map.bind(chord, action);
        }
    }
}

pub fn load_keybindings(config_dir: &Path) -> KeybindingMap {
    let mut map = default_keybindings();
    let Some((content, is_toml)) = find_keybindings_content(config_dir) else {
        return map;
    };
    for (id, entry) in parse_config_entries(&content, is_toml) {
        if let Some(action) = KeyAction::from_id(&id) {
            apply_config_entry(&mut map, action, entry);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_keybindings_include_model_select_and_cycle() {
        let map = default_keybindings();
        let ctrl_l = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL);
        assert_eq!(map.get_action(&ctrl_l), Some(KeyAction::AppModelSelect));

        let ctrl_p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert_eq!(map.get_action(&ctrl_p), Some(KeyAction::AppModelCycleForward));

        let shift_ctrl_p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::SHIFT | KeyModifiers::CONTROL);
        assert_eq!(map.get_action(&shift_ctrl_p), Some(KeyAction::AppModelCycleBackward));
    }

    #[test]
    fn custom_toml_overrides_keybindings() {
        let temp = tempfile::tempdir().unwrap();
        let toml_file = temp.path().join("keybindings.toml");
        std::fs::write(
            &toml_file,
            r#"
"app.model.select" = "ctrl+m"
"tui.editor.deleteWordBackward" = ["ctrl+w", "alt+backspace"]
"app.thinking.cycle" = []
"#,
        )
        .unwrap();

        let map = load_keybindings(temp.path());
        let ctrl_m = KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL);
        assert_eq!(map.get_action(&ctrl_m), Some(KeyAction::AppModelSelect));

        let ctrl_l = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL);
        assert_eq!(map.get_action(&ctrl_l), None);

        let shift_tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT);
        assert_eq!(map.get_action(&shift_tab), None);
    }
}
