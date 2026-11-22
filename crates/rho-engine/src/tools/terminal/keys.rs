/// The bytes a terminal sends for the key called `name` (case-insensitive), or `None` for an unknown name.
/// Cursor keys use the SS3 form while the program has enabled application cursor mode (DECCKM).
pub fn key_bytes(name: &str, application_cursor: bool) -> Option<Vec<u8>> {
    if let Some(byte) = control_byte(name) {
        return Some(vec![byte]);
    }
    let lower = name.to_ascii_lowercase();
    if let Some(code) = cursor_code(&lower) {
        let prefix = if application_cursor { "\x1bO" } else { "\x1b[" };
        return Some(format!("{prefix}{code}").into_bytes());
    }
    let sequence = match lower.as_str() {
        "enter" => "\r",
        "esc" => "\x1b",
        "tab" => "\t",
        "backspace" => "\x7f",
        "pageup" => "\x1b[5~",
        "pagedown" => "\x1b[6~",
        "delete" => "\x1b[3~",
        "f1" => "\x1bOP",
        "f2" => "\x1bOQ",
        "f3" => "\x1bOR",
        "f4" => "\x1bOS",
        "f5" => "\x1b[15~",
        "f6" => "\x1b[17~",
        "f7" => "\x1b[18~",
        "f8" => "\x1b[19~",
        "f9" => "\x1b[20~",
        "f10" => "\x1b[21~",
        "f11" => "\x1b[23~",
        "f12" => "\x1b[24~",
        _ => return None,
    };
    Some(sequence.as_bytes().to_vec())
}

fn control_byte(name: &str) -> Option<u8> {
    let letter = name.strip_prefix("C-").or_else(|| name.strip_prefix("c-"))?;
    match letter.as_bytes() {
        [byte] if byte.is_ascii_alphabetic() => Some(byte.to_ascii_lowercase() & 0x1f),
        _ => None,
    }
}

fn cursor_code(lower: &str) -> Option<char> {
    match lower {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    }
}

/// The bytes for `text` followed by `keys`, or the first unknown key name.
pub fn input_bytes(text: &str, keys: &[String], application_cursor: bool) -> Result<Vec<u8>, String> {
    let mut bytes = text.as_bytes().to_vec();
    for key in keys {
        let encoded = key_bytes(key, application_cursor).ok_or_else(|| {
            format!(
                "unknown key '{key}'; use Enter, Esc, Tab, Backspace, Up, Down, Left, Right, Home, End, PageUp, PageDown, Delete, C-a..C-z, or F1..F12"
            )
        })?;
        bytes.extend(encoded);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys_encode_to_xterm_sequences() {
        let cases: &[(&str, &[u8])] = &[
            ("Enter", b"\r"),
            ("esc", b"\x1b"),
            ("Tab", b"\t"),
            ("Backspace", b"\x7f"),
            ("Delete", b"\x1b[3~"),
            ("PageUp", b"\x1b[5~"),
            ("PageDown", b"\x1b[6~"),
            ("F1", b"\x1bOP"),
            ("F4", b"\x1bOS"),
            ("F5", b"\x1b[15~"),
            ("F12", b"\x1b[24~"),
            ("C-a", &[0x01]),
            ("C-c", &[0x03]),
            ("c-Z", &[0x1a]),
        ];
        for (name, expected) in cases {
            assert_eq!(key_bytes(name, false).as_deref(), Some(*expected), "{name}");
        }
    }

    #[test]
    fn cursor_keys_follow_application_cursor_mode() {
        assert_eq!(key_bytes("Up", false).unwrap(), b"\x1b[A");
        assert_eq!(key_bytes("Up", true).unwrap(), b"\x1bOA");
        assert_eq!(key_bytes("Left", false).unwrap(), b"\x1b[D");
        assert_eq!(key_bytes("Home", true).unwrap(), b"\x1bOH");
        assert_eq!(key_bytes("End", false).unwrap(), b"\x1b[F");
    }

    #[test]
    fn unknown_keys_are_rejected() {
        for name in ["", "C-", "C-1", "C-ab", "F13", "Space"] {
            assert_eq!(key_bytes(name, false), None, "{name}");
        }
        let error = input_bytes("x", &["Enter".into(), "Hyper".into()], false).unwrap_err();
        assert!(error.starts_with("unknown key 'Hyper'"), "{error}");
    }

    #[test]
    fn input_sends_text_before_keys() {
        let bytes = input_bytes(":wq", &["Enter".into()], false).unwrap();
        assert_eq!(bytes, b":wq\r");
    }
}
