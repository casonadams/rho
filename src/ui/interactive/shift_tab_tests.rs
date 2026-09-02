#[cfg(test)]
mod tests {
    use crate::ui::interactive::{InputAction, map_key};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn test_shift_tab_triggers_thinking_cycle() {
        // Test with KeyCode::Tab + KeyModifiers::SHIFT
        let event1 = KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT);
        assert_eq!(map_key(event1), InputAction::ThinkingCycle);

        // Test with KeyCode::BackTab + KeyModifiers::NONE (standard terminal Shift+Tab representation)
        let event2 = KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE);
        assert_eq!(map_key(event2), InputAction::ThinkingCycle);

        // Test with KeyCode::BackTab + KeyModifiers::SHIFT
        let event3 = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(map_key(event3), InputAction::ThinkingCycle);
    }
}
