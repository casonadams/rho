pub use rho_ui_core::keymap::{parse_key_chord, parse_key_code};

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

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
            assert_eq!(
                parse_key_chord(input),
                Some(rho_ui_core::keymap::KeyChord::new(code, mods))
            );
        }
    }
}
