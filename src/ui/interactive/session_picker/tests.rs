use super::{PickerAction, format_relative_time, picker_action, session_modal};
use crate::ui::interactive::ModalState;
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
fn picker_action_navigates_and_filters() {
    let mut modal = modal();
    picker_action(&mut modal, &key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(modal.selected, 1);
    picker_action(&mut modal, &key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(modal.selected, 0);

    for c in "sec".chars() {
        picker_action(&mut modal, &key(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(modal.filter_query, "sec");
    assert_eq!(modal.options.len(), 1);
    assert_eq!(
        modal.selected_option().and_then(|o| o.description.as_deref()),
        Some("bbb-222")
    );

    picker_action(&mut modal, &key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(modal.filter_query, "se");
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
fn picker_action_esc_and_ctrl_c_cancel_and_ctrl_d_is_ignored() {
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
    let mut state = modal();
    picker_action(&mut state, &key(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert_eq!(state.filter_query, "", "ctrl+d is a no-op in the startup picker");
}

#[test]
fn relative_time_buckets() {
    let now = Utc::now();
    assert_eq!(format_relative_time(now), "just now");
    assert_eq!(format_relative_time(now - ChronoDuration::minutes(5)), "5m ago");
    assert_eq!(format_relative_time(now - ChronoDuration::hours(3)), "3h ago");
    assert_eq!(format_relative_time(now - ChronoDuration::days(2)), "2d ago");
    assert_eq!(format_relative_time(now - ChronoDuration::days(10)), "10d ago");
    assert!(format_relative_time(now - ChronoDuration::days(45)).contains('-'));
}
