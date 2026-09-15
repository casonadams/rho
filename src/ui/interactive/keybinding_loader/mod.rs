use std::collections::HashMap;
use std::path::Path;

use super::key_parser::parse_key_chord;
use super::keymap::{KeyAction, KeybindingMap};

#[cfg(test)]
mod tests;

pub use rho_ui_core::keymap::default_keybindings;

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
