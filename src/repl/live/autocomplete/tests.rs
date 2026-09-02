use crate::repl::interactive::CompletionSet;
use crate::ui::interactive::{InteractiveState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io;

use super::*;

struct MockTerminal;

impl TerminalBackend for MockTerminal {
    fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
        Ok(())
    }
    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((80, 24))
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn move_up(&mut self, _rows: usize) -> io::Result<()> {
        Ok(())
    }
    fn move_down(&mut self, _rows: usize) -> io::Result<()> {
        Ok(())
    }
    fn move_to_column(&mut self, _col: usize) -> io::Result<()> {
        Ok(())
    }
    fn clear_line(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn write_text(&mut self, _text: &str) -> io::Result<()> {
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn test_autocomplete_trigger_and_selection() {
    let completions = CompletionSet::from_sources(crate::repl::interactive::CompletionSources::default());
    let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();

    // 1. Initially closed
    assert!(!controller.state().autocomplete.visible);

    // 2. Insert "/" -> opens autocomplete
    controller.state_mut().editor_mut().insert('/');
    update_autocomplete_state_generic(&mut controller, &completions);
    assert!(controller.state().autocomplete.visible);
    assert!(!controller.state().autocomplete.items.is_empty());
    assert_eq!(controller.state().autocomplete.selected, 0);

    // 3. Arrow Down moves selection
    let down_key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    let res = handle_autocomplete_key_generic(&mut controller, &completions, down_key);
    assert!(matches!(res, AutocompleteKeyResult::Handled));
    assert_eq!(controller.state().autocomplete.selected, 1);

    // 4. Enter accepts selection and closes menu
    let selected_val = controller.state().autocomplete.selected_item().unwrap().value.clone();
    let enter_key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let res = handle_autocomplete_key_generic(&mut controller, &completions, enter_key);
    assert!(matches!(res, AutocompleteKeyResult::Handled));
    assert!(!controller.state().autocomplete.visible);
    assert_eq!(controller.state().editor().text(), format!("{selected_val} "));

    // 5. Esc closes menu without clearing
    controller.state_mut().editor_mut().set_text("/");
    update_autocomplete_state_generic(&mut controller, &completions);
    assert!(controller.state().autocomplete.visible);

    let esc_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    let res = handle_autocomplete_key_generic(&mut controller, &completions, esc_key);
    assert!(matches!(res, AutocompleteKeyResult::Handled));
    assert!(!controller.state().autocomplete.visible);
    assert_eq!(controller.state().editor().text(), "/");
}
