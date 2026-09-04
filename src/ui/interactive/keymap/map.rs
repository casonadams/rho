use std::collections::HashMap;

use crossterm::event::KeyEvent;

use super::{action::KeyAction, chord::KeyChord};

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
