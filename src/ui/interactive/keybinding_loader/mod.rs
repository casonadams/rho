use std::collections::HashMap;
use std::path::Path;

use super::key_parser::parse_key_chord;
use super::keymap::{KeyAction, KeybindingMap};

#[cfg(test)]
mod tests;

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
    ("app.blockStyle.toggle", &["ctrl+s"]),
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
