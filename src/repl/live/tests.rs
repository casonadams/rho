use super::live_ui_supported;
use super::navigation::{navigate_history_next, navigate_history_previous};
use crate::repl::interactive::InteractiveHistory;
use crate::ui::interactive::{InteractiveState, TerminalBackend, TerminalController};
use std::{fs, io};

struct HistoryTerminal;

impl TerminalBackend for HistoryTerminal {
    fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
        Ok(())
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((20, 24))
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

    fn move_to_column(&mut self, _column: usize) -> io::Result<()> {
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
fn live_ui_requires_both_terminal_streams() {
    assert!(live_ui_supported(true, true));
    assert!(!live_ui_supported(true, false));
    assert!(!live_ui_supported(false, true));
    assert!(!live_ui_supported(false, false));
}

#[test]
fn active_history_navigation_uses_visual_boundaries_and_restores_the_draft() {
    let path = std::env::temp_dir().join(format!("rho-live-history-{}.txt", uuid::Uuid::new_v4()));
    let mut history = InteractiveHistory::with_file(10, path.clone()).unwrap();
    history.record("older").unwrap();
    history.record("newer\nsecond").unwrap();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("draft\nline");

    assert!(navigate_history_previous(&mut controller, &mut history));
    assert_eq!(controller.state().editor().text(), "draft\nline");
    assert!(navigate_history_previous(&mut controller, &mut history));
    assert_eq!(controller.state().editor().text(), "newer\nsecond");
    assert!(navigate_history_previous(&mut controller, &mut history));
    assert_eq!(controller.state().editor().text(), "newer\nsecond");
    assert!(navigate_history_previous(&mut controller, &mut history));
    assert_eq!(controller.state().editor().text(), "older");
    assert!(navigate_history_next(&mut controller, &mut history));
    assert_eq!(controller.state().editor().text(), "newer\nsecond");
    assert!(navigate_history_next(&mut controller, &mut history));
    assert_eq!(controller.state().editor().text(), "draft\nline");

    drop(controller);
    drop(history);
    fs::remove_file(path).unwrap();
}
