use crossterm::event::{KeyCode, KeyModifiers};

use super::keymap::KeyChord;

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

const SINGLE_CHAR_KEYS: &[(&str, KeyCode)] = &[
    ("enter", KeyCode::Enter),
    ("return", KeyCode::Enter),
    ("esc", KeyCode::Esc),
    ("escape", KeyCode::Esc),
    ("backspace", KeyCode::Backspace),
    ("tab", KeyCode::Tab),
    ("backtab", KeyCode::BackTab),
    ("delete", KeyCode::Delete),
    ("del", KeyCode::Delete),
    ("insert", KeyCode::Insert),
    ("ins", KeyCode::Insert),
    ("up", KeyCode::Up),
    ("down", KeyCode::Down),
    ("left", KeyCode::Left),
    ("right", KeyCode::Right),
    ("home", KeyCode::Home),
    ("end", KeyCode::End),
    ("pageup", KeyCode::PageUp),
    ("pagedown", KeyCode::PageDown),
    ("space", KeyCode::Char(' ')),
];

fn parse_key_code(raw: &str) -> Option<KeyCode> {
    let lower = raw.to_ascii_lowercase();
    if let Some((_, code)) = SINGLE_CHAR_KEYS.iter().find(|(name, _)| *name == lower) {
        return Some(*code);
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok())
        && (1..=12).contains(&n)
    {
        return Some(KeyCode::F(n));
    }
    if lower.chars().count() == 1 {
        return Some(KeyCode::Char(lower.chars().next().unwrap()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_and_modified_keys() {
        let cases = [
            ("ctrl+l", KeyCode::Char('l'), KeyModifiers::CONTROL),
            (
                "shift+ctrl+p",
                KeyCode::Char('p'),
                KeyModifiers::SHIFT | KeyModifiers::CONTROL,
            ),
            ("alt+enter", KeyCode::Enter, KeyModifiers::ALT),
            ("shift+tab", KeyCode::Tab, KeyModifiers::SHIFT),
            ("escape", KeyCode::Esc, KeyModifiers::NONE),
            ("ctrl+-", KeyCode::Char('-'), KeyModifiers::CONTROL),
        ];
        for (input, code, mods) in cases {
            assert_eq!(parse_key_chord(input), Some(KeyChord::new(code, mods)));
        }
    }
}
