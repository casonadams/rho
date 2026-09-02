use crossterm::event::{KeyCode, KeyModifiers};

use super::keymap::KeyChord;

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
        match lower.as_str() {
            "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
            "alt" | "opt" | "option" => modifiers |= KeyModifiers::ALT,
            "shift" => modifiers |= KeyModifiers::SHIFT,
            "super" | "cmd" | "command" => modifiers |= KeyModifiers::SUPER,
            _ => {
                key_part = part;
            }
        }
    }

    let code = parse_key_code(key_part)?;
    Some(KeyChord::new(code, modifiers))
}

fn parse_key_code(raw: &str) -> Option<KeyCode> {
    let lower = raw.to_ascii_lowercase();
    match lower.as_str() {
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
        "f1" => Some(KeyCode::F(1)),
        "f2" => Some(KeyCode::F(2)),
        "f3" => Some(KeyCode::F(3)),
        "f4" => Some(KeyCode::F(4)),
        "f5" => Some(KeyCode::F(5)),
        "f6" => Some(KeyCode::F(6)),
        "f7" => Some(KeyCode::F(7)),
        "f8" => Some(KeyCode::F(8)),
        "f9" => Some(KeyCode::F(9)),
        "f10" => Some(KeyCode::F(10)),
        "f11" => Some(KeyCode::F(11)),
        "f12" => Some(KeyCode::F(12)),
        c if c.chars().count() == 1 => Some(KeyCode::Char(c.chars().next().unwrap())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_and_modified_keys() {
        assert_eq!(
            parse_key_chord("ctrl+l"),
            Some(KeyChord::new(KeyCode::Char('l'), KeyModifiers::CONTROL))
        );
        assert_eq!(
            parse_key_chord("shift+ctrl+p"),
            Some(KeyChord::new(
                KeyCode::Char('p'),
                KeyModifiers::SHIFT | KeyModifiers::CONTROL
            ))
        );
        assert_eq!(
            parse_key_chord("alt+enter"),
            Some(KeyChord::new(KeyCode::Enter, KeyModifiers::ALT))
        );
        assert_eq!(
            parse_key_chord("shift+tab"),
            Some(KeyChord::new(KeyCode::Tab, KeyModifiers::SHIFT))
        );
        assert_eq!(
            parse_key_chord("escape"),
            Some(KeyChord::new(KeyCode::Esc, KeyModifiers::NONE))
        );
        assert_eq!(
            parse_key_chord("ctrl+-"),
            Some(KeyChord::new(KeyCode::Char('-'), KeyModifiers::CONTROL))
        );
    }
}
