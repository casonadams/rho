use std::collections::HashMap;
use std::path::Path;

use super::key_parser::parse_key_chord;
use super::keymap::{KeyAction, KeybindingMap};

pub fn default_keybindings() -> KeybindingMap {
    let mut map = KeybindingMap::new();

    let defaults: &[(&str, &[&str])] = &[
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
        ("tui.select.cancel", &["escape", "ctrl+c"]),
    ];

    for (id, keys) in defaults {
        if let Some(action) = KeyAction::from_id(id) {
            for key_str in *keys {
                if let Some(chord) = parse_key_chord(key_str) {
                    map.bind(chord, action);
                }
            }
        }
    }

    map
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum ConfigEntry {
    Single(String),
    Multiple(Vec<String>),
}

pub fn load_keybindings(config_dir: &Path) -> KeybindingMap {
    let mut map = default_keybindings();

    let toml_path = config_dir.join("keybindings.toml");
    let json_path = config_dir.join("keybindings.json");
    let pi_json_path = dirs::home_dir().map(|h| h.join(".pi/agent/keybindings.json"));

    let content_and_is_toml = if toml_path.exists() {
        std::fs::read_to_string(&toml_path).ok().map(|c| (c, true))
    } else if json_path.exists() {
        std::fs::read_to_string(&json_path).ok().map(|c| (c, false))
    } else if let Some(ref pi_path) = pi_json_path
        && pi_path.exists()
    {
        std::fs::read_to_string(pi_path).ok().map(|c| (c, false))
    } else {
        None
    };

    let Some((content, is_toml)) = content_and_is_toml else {
        return map;
    };

    let entries: HashMap<String, ConfigEntry> = if is_toml {
        toml::from_str(&content).unwrap_or_default()
    } else {
        serde_json::from_str(&content).unwrap_or_default()
    };

    for (id, entry) in entries {
        if let Some(action) = KeyAction::from_id(&id) {
            map.unbind_action(action);
            match entry {
                ConfigEntry::Single(k) => {
                    if let Some(chord) = parse_key_chord(&k) {
                        map.bind(chord, action);
                    }
                }
                ConfigEntry::Multiple(keys) => {
                    for k in keys {
                        if let Some(chord) = parse_key_chord(&k) {
                            map.bind(chord, action);
                        }
                    }
                }
            }
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
