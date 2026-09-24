use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};
use crate::ui::render::formatters::format_relative_time;
use crate::ui::theme::Theme;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::error::Result;
use rho_harness_core::session::{SessionManager, SessionSummary};
use std::path::Path;

pub fn prompt_session_picker(sessions_dir: &Path, theme: &Theme) -> Result<Option<String>> {
    let summaries = SessionManager::list_session_summaries(sessions_dir)?;
    if summaries.is_empty() {
        return Ok(None);
    }

    let mut controller = TerminalController::stdout(crate::ui::interactive::InteractiveState::default())?;
    controller.set_theme(theme.clone())?;
    controller.state_mut().push_modal(session_modal(&summaries));
    controller.redraw()?;

    key_loop(&mut controller)
}

pub fn session_modal(summaries: &[SessionSummary]) -> ModalState {
    let options = summaries
        .iter()
        .map(|s| {
            let title = s.name.as_deref().unwrap_or(&s.preview);
            let time = format_relative_time(s.last_modified);
            let label = format!("{title} ({} | {} turns | {time})", s.session_id, s.turn_count);
            ModalOption::new(label, Some(s.session_id.clone()))
        })
        .collect();
    ModalState::new("Resume Session", "", options).with_search(true)
}

#[derive(Debug)]
enum PickerAction {
    Repaint,
    Select(String),
    Cancel,
}

fn picker_action(modal: &mut ModalState, key: &KeyEvent) -> PickerAction {
    match key.code {
        KeyCode::Up | KeyCode::BackTab => {
            modal.select_previous();
            PickerAction::Repaint
        }
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
            modal.select_previous();
            PickerAction::Repaint
        }
        KeyCode::Down | KeyCode::Tab => {
            modal.select_next();
            PickerAction::Repaint
        }
        KeyCode::Enter => {
            let session_id = modal
                .selected_option()
                .and_then(|o| o.description.clone())
                .unwrap_or_default();
            PickerAction::Select(session_id)
        }
        KeyCode::Esc => PickerAction::Cancel,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => PickerAction::Cancel,
        KeyCode::Backspace => {
            let mut query = modal.filter_query.clone();
            query.pop();
            modal.set_filter(&query);
            PickerAction::Repaint
        }
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            let mut query = modal.filter_query.clone();
            query.push(c);
            modal.set_filter(&query);
            PickerAction::Repaint
        }
        _ => PickerAction::Repaint,
    }
}

fn handle_resize<B: TerminalBackend>(controller: &mut TerminalController<B>, cols: u16, rows: u16) -> Result<()> {
    let _ = controller.resize_to(usize::from(cols), usize::from(rows))? || controller.refresh_size()?;
    Ok(())
}

fn handle_key_event<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: &KeyEvent,
) -> Result<Option<Option<String>>> {
    let _ = controller.refresh_size()?;
    if key.kind != crossterm::event::KeyEventKind::Press {
        return Ok(None);
    }
    let Some(modal) = controller.state_mut().active_modal_mut() else {
        return Ok(Some(None));
    };
    match picker_action(modal, key) {
        PickerAction::Repaint => {
            controller.redraw()?;
            Ok(None)
        }
        PickerAction::Select(session_id) => {
            controller.state_mut().pop_modal();
            controller.redraw()?;
            Ok(Some(Some(session_id)))
        }
        PickerAction::Cancel => {
            controller.state_mut().pop_modal();
            controller.redraw()?;
            Ok(Some(None))
        }
    }
}

fn handle_picker_event<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    event: &Event,
) -> Result<Option<Option<String>>> {
    match event {
        Event::Resize(cols, rows) => {
            handle_resize(controller, *cols, *rows)?;
            Ok(None)
        }
        Event::Key(key) => handle_key_event(controller, key),
        _ => Ok(None),
    }
}

fn run_event_loop<B: TerminalBackend, F>(
    controller: &mut TerminalController<B>,
    mut next_event: F,
) -> Result<Option<String>>
where
    F: FnMut() -> Result<Event>,
{
    loop {
        let event = next_event()?;
        if let Some(outcome) = handle_picker_event(controller, &event)? {
            return Ok(outcome);
        }
    }
}

fn key_loop(controller: &mut TerminalController<crate::ui::interactive::CrosstermBackend>) -> Result<Option<String>> {
    run_event_loop(controller, || crossterm::event::read().map_err(Into::into))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as ChronoDuration, Utc};
    use crossterm::event::{KeyCode, KeyEvent, KeyEventState, KeyModifiers};
    use rho_harness_core::session::SessionSummary;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: crossterm::event::KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn summaries() -> Vec<SessionSummary> {
        let now = Utc::now();
        vec![
            SessionSummary {
                session_id: "aaa-111".into(),
                name: Some("First".into()),
                created_at: now - ChronoDuration::minutes(10),
                last_modified: now - ChronoDuration::minutes(5),
                turn_count: 3,
                preview: "first preview".into(),
            },
            SessionSummary {
                session_id: "bbb-222".into(),
                name: None,
                created_at: now - ChronoDuration::hours(2),
                last_modified: now - ChronoDuration::minutes(90),
                turn_count: 9,
                preview: "second preview".into(),
            },
        ]
    }

    fn modal() -> ModalState {
        session_modal(&summaries())
    }

    #[test]
    fn session_modal_labels_carry_the_session_id_in_the_description() {
        let modal = modal();
        let labels: Vec<(&str, &str)> = modal
            .all_options
            .iter()
            .map(|o| (o.label.as_str(), o.description.as_deref().unwrap_or("")))
            .collect();
        assert!(labels[0].0.contains("First"));
        assert_eq!(labels[0].1, "aaa-111");
        assert_eq!(labels[1].1, "bbb-222");
        assert!(labels[1].0.contains("second preview"));
    }

    #[test]
    fn picker_action_navigates() {
        let mut modal = modal();
        picker_action(&mut modal, &key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(modal.selected, 1);
        picker_action(&mut modal, &key(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(modal.selected, 0);
        picker_action(&mut modal, &key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.selected, 1);
        picker_action(&mut modal, &key(KeyCode::Tab, KeyModifiers::SHIFT));
        assert_eq!(modal.selected, 0);
        picker_action(&mut modal, &key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.selected, 1);
        picker_action(&mut modal, &key(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(modal.selected, 0);
    }

    #[test]
    fn picker_action_filters_query() {
        let mut modal = modal();
        for c in "sec".chars() {
            picker_action(&mut modal, &key(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(modal.filter_query, "sec");
        assert_eq!(modal.options.len(), 1);
        let desc = modal.selected_option().and_then(|o| o.description.as_deref());
        assert_eq!(desc, Some("bbb-222"));
    }

    #[test]
    fn picker_action_backspace_removes_filter() {
        let mut modal = modal();
        picker_action(&mut modal, &key(KeyCode::Char('s'), KeyModifiers::NONE));
        picker_action(&mut modal, &key(KeyCode::Char('e'), KeyModifiers::NONE));
        picker_action(&mut modal, &key(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(modal.filter_query, "s");
    }

    #[test]
    fn picker_action_enter_selects_the_highlighted_session() {
        let mut modal = modal();
        picker_action(&mut modal, &key(KeyCode::Down, KeyModifiers::NONE));
        match picker_action(&mut modal, &key(KeyCode::Enter, KeyModifiers::NONE)) {
            PickerAction::Select(id) => assert_eq!(id, "bbb-222"),
            other => panic!("expected selection, got {other:?}"),
        }
    }

    #[test]
    fn picker_action_cancels() {
        let mut state = modal();
        assert!(matches!(
            picker_action(&mut state, &key(KeyCode::Esc, KeyModifiers::NONE)),
            PickerAction::Cancel
        ));
        let mut state = modal();
        assert!(matches!(
            picker_action(&mut state, &key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            PickerAction::Cancel
        ));
    }

    #[test]
    fn picker_action_ctrl_d_is_ignored() {
        let mut state = modal();
        picker_action(&mut state, &key(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert_eq!(state.filter_query, "");
    }

    #[test]
    fn run_event_loop_selects_session() {
        use crate::ui::interactive::InteractiveState;
        use crate::ui::interactive::controller::tests::fake::FakeTerminal;

        let (backend, _, _) = FakeTerminal::new(80);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        controller.state_mut().push_modal(modal());

        let mut events = vec![
            Event::Resize(80, 24),
            Event::FocusGained,
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                modifiers: KeyModifiers::NONE,
                kind: crossterm::event::KeyEventKind::Release,
                state: KeyEventState::NONE,
            }),
            Event::Key(key(KeyCode::Down, KeyModifiers::NONE)),
            Event::Key(key(KeyCode::Enter, KeyModifiers::NONE)),
        ]
        .into_iter();

        let result = run_event_loop(&mut controller, || Ok(events.next().unwrap())).unwrap();
        assert_eq!(result, Some("bbb-222".to_string()));
    }

    #[test]
    fn run_event_loop_cancels() {
        use crate::ui::interactive::InteractiveState;
        use crate::ui::interactive::controller::tests::fake::FakeTerminal;

        let (backend, _, _) = FakeTerminal::new(80);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
        controller.state_mut().push_modal(modal());

        let mut events = vec![Event::Key(key(KeyCode::Esc, KeyModifiers::NONE))].into_iter();

        let result = run_event_loop(&mut controller, || Ok(events.next().unwrap())).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn run_event_loop_exits_when_no_active_modal() {
        use crate::ui::interactive::InteractiveState;
        use crate::ui::interactive::controller::tests::fake::FakeTerminal;

        let (backend, _, _) = FakeTerminal::new(80);
        let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();

        let mut events = vec![Event::Key(key(KeyCode::Enter, KeyModifiers::NONE))].into_iter();

        let result = run_event_loop(&mut controller, || Ok(events.next().unwrap())).unwrap();
        assert_eq!(result, None);
    }
}
