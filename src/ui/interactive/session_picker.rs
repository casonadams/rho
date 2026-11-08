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
    run_picker(&mut controller, &summaries, || {
        crossterm::event::read().map_err(Into::into)
    })
}

fn run_picker<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    summaries: &[SessionSummary],
    next_event: impl FnMut() -> Result<Event>,
) -> Result<Option<String>> {
    controller.state_mut().push_modal(session_modal(summaries));
    controller.redraw()?;
    run_event_loop(controller, next_event)
}

pub fn session_modal(summaries: &[SessionSummary]) -> ModalState {
    resume_modal(resume_options(summaries, session_row), "")
}

pub(crate) fn session_row(item: &SessionSummary) -> (String, String) {
    let title = item.name.as_deref().unwrap_or(&item.preview);
    let mut parts = vec![
        format_relative_time(item.last_modified),
        format!("{} turns", item.turn_count),
    ];
    if item.name.is_some() {
        parts.push(item.preview.clone());
    }
    parts.push(item.session_id.clone());
    (title.to_string(), parts.join("\t"))
}

pub(crate) fn resume_options(
    summaries: &[SessionSummary],
    row: impl Fn(&SessionSummary) -> (String, String),
) -> Vec<ModalOption> {
    summaries
        .iter()
        .filter(|s| s.turn_count > 0 || s.name.is_some())
        .map(|s| {
            let (label, description) = row(s);
            ModalOption::new(label, Some(description)).with_value(s.session_id.clone())
        })
        .collect()
}

pub(crate) fn resume_modal(options: Vec<ModalOption>, subtitle: &str) -> ModalState {
    ModalState::new("Resume Session", subtitle, options).with_search(true)
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
                .and_then(|o| o.value.clone())
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
    fn session_modal_rows_are_titled_by_name_or_first_message() {
        let modal = modal();
        let rows: Vec<(&str, &str)> = modal
            .all_options
            .iter()
            .map(|o| (o.label.as_str(), o.description.as_deref().unwrap_or("")))
            .collect();
        assert_eq!(rows[0].0, "First");
        assert!(rows[0].1.ends_with("3 turns\tfirst preview\taaa-111"), "{}", rows[0].1);
        assert_eq!(rows[1].0, "second preview");
        assert!(rows[1].1.ends_with("9 turns\tbbb-222"), "{}", rows[1].1);
    }

    #[test]
    fn session_modal_options_carry_the_id_as_value() {
        let modal = modal();
        let values: Vec<Option<&str>> = modal.all_options.iter().map(|o| o.value.as_deref()).collect();
        assert_eq!(values, [Some("aaa-111"), Some("bbb-222")]);
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
        assert!(desc.is_some_and(|d| d.ends_with("bbb-222")));
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
    fn picker_event_loop_outcomes() {
        use crate::ui::interactive::InteractiveState;
        use crate::ui::interactive::controller::tests::fake::FakeTerminal;

        let press = |code| Event::Key(key(code, KeyModifiers::NONE));
        let release_down = Event::Key(KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            kind: crossterm::event::KeyEventKind::Release,
            state: KeyEventState::NONE,
        });
        let cases: Vec<(bool, Vec<Event>, Option<&str>)> = vec![
            (
                true,
                vec![
                    Event::Resize(80, 24),
                    Event::FocusGained,
                    release_down,
                    press(KeyCode::Down),
                    press(KeyCode::Enter),
                ],
                Some("bbb-222"),
            ),
            (true, vec![press(KeyCode::Down), press(KeyCode::Enter)], Some("bbb-222")),
            (true, vec![press(KeyCode::Esc)], None),
            (false, vec![press(KeyCode::Enter)], None),
        ];
        for (picker, events, expected) in cases {
            let (backend, _, _) = FakeTerminal::new(80);
            let mut controller = TerminalController::new(backend, InteractiveState::default()).unwrap();
            let mut events = events.into_iter();
            let next_event = || -> Result<Event> { Ok(events.next().unwrap()) };
            let result = if picker {
                run_picker(&mut controller, &summaries(), next_event)
            } else {
                run_event_loop(&mut controller, next_event)
            };
            assert_eq!(result.unwrap(), expected.map(str::to_string));
        }
    }

    #[test]
    fn prompt_session_picker_without_sessions_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let result = prompt_session_picker(dir.path(), &Theme::default()).unwrap();
        assert_eq!(result, None);
    }
}
