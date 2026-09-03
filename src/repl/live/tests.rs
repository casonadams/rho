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

#[test]
fn live_batch_flushes_tool_end_with_transcript_without_intermediate_redraw() {
    let mut batch = super::batch::LiveBatch::new();
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    controller
        .start_tool(crate::ui::interactive::ToolStartRequest {
            name: "bash".into(),
            args_summary: "cargo test".into(),
            preview: None,
        })
        .unwrap();

    batch
        .enqueue(&mut controller, crate::ui::interactive::UiEvent::ToolEnd)
        .unwrap();
    batch
        .enqueue(
            &mut controller,
            crate::ui::interactive::UiEvent::Transcript(crate::ui::interactive::TranscriptItem::Tool(
                crate::ui::interactive::ToolItem {
                    name: "bash".into(),
                    arguments: serde_json::json!({"command": "cargo test"}),
                    is_error: false,
                    output: "all tests passed".into(),
                    output_summary: "ok".into(),
                    duration_ms: Some(50),
                },
            )),
        )
        .unwrap();

    batch.flush(&mut controller, false).unwrap();
    assert_eq!(controller.transcript().len(), 1);
}

#[test]
fn model_selector_modal_filtering_and_selection() {
    let config = rho_harness_core::config::Config::default();
    let auth_store = crate::auth::AuthStore::load(&config.auth_file).unwrap_or_default();
    let session = crate::repl::ReplSession::new(config, auth_store, None);
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();

    // Open model selector
    super::modal::open_model_selector(&session, &mut controller);
    assert_eq!(controller.state().active_modal().unwrap().title, "Select Model");

    // Filter by "sonnet"
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('s'),
        crossterm::event::KeyModifiers::NONE,
    );
    let res = super::modal::handle_modal_key(&mut controller, key, &mut None).unwrap();
    assert_eq!(res, super::modal::ModalKeyResult::Handled);
    assert_eq!(controller.state().active_modal().unwrap().filter_query, "s");

    // Filter with "claude"
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.set_filter("claude");
    }
    let modal = controller.state().active_modal().unwrap();
    assert!(modal.options.iter().any(|o| o.label.contains("claude")));

    // Select with Enter
    let enter_key =
        crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
    let res = super::modal::handle_modal_key(&mut controller, enter_key, &mut None).unwrap();
    match res {
        super::modal::ModalKeyResult::ModelSelected {
            model,
            provider,
            save_as_default,
        } => {
            assert!(model.contains("claude"));
            assert!(!provider.is_empty());
            assert!(!save_as_default);
        }
        _ => panic!("expected ModelSelected result"),
    }
    assert!(controller.state().active_modal().is_none());
}

#[test]
fn test_paste_event_collapses_in_interactive_state() {
    let mut controller = TerminalController::new(HistoryTerminal, InteractiveState::default()).unwrap();
    let lines = (1..=15).map(|i| format!("code {i}")).collect::<Vec<_>>().join("\n");
    controller
        .state_mut()
        .apply(crate::ui::interactive::UiAction::Paste(lines));
    assert_eq!(controller.state().editor().text(), "[paste #1 +15 lines]");
}
