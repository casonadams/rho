use crate::ui::interactive::{ModalOption, ModalState, TerminalController};
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

fn handle_filter_key(modal: &mut ModalState, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Backspace => {
            let mut query = modal.filter_query.clone();
            query.pop();
            modal.set_filter(&query);
            true
        }
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            let mut query = modal.filter_query.clone();
            query.push(c);
            modal.set_filter(&query);
            true
        }
        _ => false,
    }
}

fn picker_enter(modal: &ModalState) -> PickerAction {
    let session_id = modal
        .selected_option()
        .and_then(|o| o.description.clone())
        .unwrap_or_default();
    PickerAction::Select(session_id)
}

fn picker_step(modal: &mut ModalState, prev: bool) -> PickerAction {
    if prev {
        modal.select_previous();
    } else {
        modal.select_next();
    }
    PickerAction::Repaint
}

fn picker_action(modal: &mut ModalState, key: &KeyEvent) -> PickerAction {
    match key.code {
        KeyCode::Up | KeyCode::BackTab => picker_step(modal, true),
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => picker_step(modal, true),
        KeyCode::Down | KeyCode::Tab => picker_step(modal, false),
        KeyCode::Enter => picker_enter(modal),
        KeyCode::Esc => PickerAction::Cancel,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => PickerAction::Cancel,
        _ => {
            handle_filter_key(modal, key);
            PickerAction::Repaint
        }
    }
}

fn key_loop(controller: &mut TerminalController<crate::ui::interactive::CrosstermBackend>) -> Result<Option<String>> {
    loop {
        match crossterm::event::read()? {
            Event::Resize(cols, rows) => {
                let _ = controller.resize_to(usize::from(cols), usize::from(rows))? || controller.refresh_size()?;
            }
            Event::Key(key) => {
                let _ = controller.refresh_size()?;
                if key.kind != crossterm::event::KeyEventKind::Press {
                    continue;
                }
                let Some(modal) = controller.state_mut().active_modal_mut() else {
                    return Ok(None);
                };
                match picker_action(modal, &key) {
                    PickerAction::Repaint => controller.redraw()?,
                    PickerAction::Select(session_id) => {
                        controller.state_mut().pop_modal();
                        controller.redraw()?;
                        return Ok(Some(session_id));
                    }
                    PickerAction::Cancel => {
                        controller.state_mut().pop_modal();
                        controller.redraw()?;
                        return Ok(None);
                    }
                }
            }
            _ => {}
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
}
