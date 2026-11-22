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
        "space" => " ",
        "esc" => "\x1b",
        "tab" => "\t",
        "s-tab" => "\x1b[Z",
        "insert" => "\x1b[2~",
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
        [byte @ (b'[' | b'\\' | b']')] => Some(byte & 0x1f),
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

/// The input modes the program has enabled, which change how keys and text are encoded.
#[derive(Debug, Clone, Copy, Default)]
pub struct InputModes {
    pub application_cursor: bool,
    pub bracketed_paste: bool,
}

const ESC: u8 = 0x1b;

/// Keys that make the tty discard queued input and echo (C-c, C-z, C-\) and so must not share a
/// write with the bytes around them.
fn flushes_tty(encoded: &[u8]) -> bool {
    matches!(encoded, [0x03 | 0x1a | 0x1c])
}

/// The writes for `text` followed by `keys`, or the first unknown key name. The stream is split
/// around keys that flush the tty queues, and after Esc so it is not read as an Alt prefix.
pub fn input_segments(text: &str, keys: &[String], modes: InputModes) -> Result<Vec<Vec<u8>>, String> {
    let mut segments = Vec::new();
    let mut current = if modes.bracketed_paste && text.contains('\n') {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.as_bytes().to_vec()
    };
    for key in keys {
        let encoded = key_bytes(key, modes.application_cursor).ok_or_else(|| {
            format!(
                "unknown key '{key}'; use Enter, Space, Esc, Tab, S-Tab, Backspace, Up, Down, Left, Right, Home, End, PageUp, PageDown, Insert, Delete, C-a..C-z, C-[, C-\\, C-], or F1..F12"
            )
        })?;
        let flushes = flushes_tty(&encoded);
        if flushes && !current.is_empty() {
            segments.push(std::mem::take(&mut current));
        }
        current.extend_from_slice(&encoded);
        if flushes || encoded == [ESC] {
            segments.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    Ok(segments)
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
            ("Space", b" "),
            ("S-Tab", b"\x1b[Z"),
            ("Insert", b"\x1b[2~"),
            ("C-[", &[0x1b]),
            ("C-\\", &[0x1c]),
            ("C-]", &[0x1d]),
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
        for name in ["", "C-", "C-1", "C-ab", "F13", "C-^"] {
            assert_eq!(key_bytes(name, false), None, "{name}");
        }
        let error = input_segments("x", &["Enter".into(), "Hyper".into()], InputModes::default()).unwrap_err();
        assert!(error.starts_with("unknown key 'Hyper'"), "{error}");
    }

    fn segments(text: &str, keys: &[&str], modes: InputModes) -> Vec<Vec<u8>> {
        let keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        input_segments(text, &keys, modes).unwrap()
    }

    #[test]
    fn input_sends_text_before_keys() {
        assert_eq!(segments(":wq", &["Enter"], InputModes::default()), [b":wq\r".to_vec()]);
    }

    #[test]
    fn tty_flushing_keys_and_esc_end_a_write() {
        let modes = InputModes::default();
        type Case<'a> = (&'a str, &'a [&'a str], &'a [&'a [u8]]);
        let cases: &[Case] = &[
            ("hello", &["Enter", "C-c"], &[b"hello\r", b"\x03"]),
            ("", &["C-c", "C-z"], &[b"\x03", b"\x1a"]),
            ("a", &["Esc", "Up"], &[b"a\x1b", b"\x1b[A"]),
            ("", &["Tab", "C-\\", "Enter"], &[b"\t", b"\x1c", b"\r"]),
            ("", &["C-d"], &[b"\x04"]),
        ];
        for (text, keys, expected) in cases {
            assert_eq!(segments(text, keys, modes), *expected, "{text} {keys:?}");
        }
    }

    #[test]
    fn multi_line_text_is_pasted_only_when_the_program_enabled_it() {
        let pasting = InputModes {
            bracketed_paste: true,
            ..InputModes::default()
        };
        assert_eq!(segments("a\nb", &[], pasting), [b"\x1b[200~a\nb\x1b[201~".to_vec()]);
        assert_eq!(segments("ab", &[], pasting), [b"ab".to_vec()]);
        assert_eq!(segments("a\nb", &[], InputModes::default()), [b"a\nb".to_vec()]);
    }
}
