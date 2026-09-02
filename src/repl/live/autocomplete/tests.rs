use crate::repl::interactive::CompletionSet;
use crate::ui::interactive::{InteractiveState, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_core::skills::{ResolvedSkill, SkillMetadata, SkillOrigin};
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
fn test_autocomplete_trigger_and_chaining_and_tab_cycle() {
    let skill1 = ResolvedSkill {
        metadata: SkillMetadata {
            name: "plan".to_string(),
            description: "Planning workflow".to_string(),
            location: "/path".to_string(),
        },
        origin: SkillOrigin::Builtin,
    };
    let skill2 = ResolvedSkill {
        metadata: SkillMetadata {
            name: "spec".to_string(),
            description: "Specification workflow".to_string(),
            location: "/path".to_string(),
        },
        origin: SkillOrigin::Builtin,
    };
    let sources = crate::repl::interactive::CompletionSources::new().with_skills(vec![skill1, skill2]);
    let completions = CompletionSet::from_sources(sources);
    let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();

    // 1. Type "/skil" -> opens autocomplete
    controller.state_mut().editor_mut().set_text("/skil");
    update_autocomplete_state_generic(&mut controller, &completions);
    assert!(controller.state().autocomplete.visible);
    assert_eq!(controller.state().autocomplete.selected_item().unwrap().value, "/skill");

    // 2. Tab completes partial text to "/skill " and immediately opens available skills
    let tab_key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    let res = handle_autocomplete_key_generic(&mut controller, &completions, tab_key);
    assert!(matches!(res, AutocompleteKeyResult::Handled));
    assert_eq!(controller.state().editor().text(), "/skill ");
    assert!(controller.state().autocomplete.visible);
    assert_eq!(controller.state().autocomplete.selected, 0);

    // 3. Tab again CYCLES to the next skill item!
    let res = handle_autocomplete_key_generic(&mut controller, &completions, tab_key);
    assert!(matches!(res, AutocompleteKeyResult::Handled));
    assert_eq!(controller.state().autocomplete.selected, 1);

    // 4. Enter accepts the currently selected skill (spec)
    let enter_key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let res = handle_autocomplete_key_generic(&mut controller, &completions, enter_key);
    assert!(matches!(res, AutocompleteKeyResult::Handled));
    assert_eq!(controller.state().editor().text(), "/skill spec ");
}
