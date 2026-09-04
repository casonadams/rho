use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

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
