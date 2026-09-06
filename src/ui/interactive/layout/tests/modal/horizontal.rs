use crate::ui::interactive::layout::modal::modal_hint;
use crate::ui::interactive::layout::modal::options::{ModalOptionsLayout, render_modal_options};
use crate::ui::interactive::{ModalMode, ModalOption, ModalState, OptionLayout};
use crate::ui::theme::Theme;

fn sample_permission_modal() -> ModalState {
    let mut modal = ModalState::new(
        "Permission Required",
        "Run echo hello",
        vec![
            ModalOption::from("Allow"),
            ModalOption::from("Edit"),
            ModalOption::from("Always"),
            ModalOption::from("Deny"),
        ],
    );
    modal.option_layout = OptionLayout::Horizontal;
    modal
}

#[test]
fn test_horizontal_options_all_fit_rendering() {
    let modal = sample_permission_modal();
    let theme = Theme::default();
    let lines = render_modal_options(
        &modal,
        ModalOptionsLayout {
            inner_width: 80,
            max_visible: 1,
            theme: &theme,
        },
    );
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert!(line.contains("Allow") && line.contains("Edit") && line.contains("Always") && line.contains("Deny"));
    assert!(line.contains('▸'));
    assert!(!line.contains('‹') && !line.contains('›'));
}

#[test]
fn test_horizontal_options_selection_marker_moves() {
    let mut modal = sample_permission_modal();
    modal.selected = 1;
    let theme = Theme::default();
    let lines = render_modal_options(
        &modal,
        ModalOptionsLayout {
            inner_width: 80,
            max_visible: 1,
            theme: &theme,
        },
    );
    let line = &lines[0];
    assert!(line.contains("▸") && line.contains("Edit"));
    assert!(!line.contains("▸ Allow"));
}

#[test]
fn test_horizontal_options_overflow_right_indicator() {
    let modal = sample_permission_modal();
    let theme = Theme::default();
    let lines = render_modal_options(
        &modal,
        ModalOptionsLayout {
            inner_width: 16,
            max_visible: 1,
            theme: &theme,
        },
    );
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert!(line.contains("Allow"));
    assert!(line.contains('›'));
    assert!(!line.contains('‹'));
}

#[test]
fn test_horizontal_options_overflow_left_indicator() {
    let mut modal = sample_permission_modal();
    modal.selected = 3;
    let theme = Theme::default();
    let lines = render_modal_options(
        &modal,
        ModalOptionsLayout {
            inner_width: 16,
            max_visible: 1,
            theme: &theme,
        },
    );
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert!(line.contains("Deny"));
    assert!(line.contains('‹'));
    assert!(!line.contains('›'));
}

#[test]
fn test_horizontal_options_overflow_both_indicators() {
    let mut modal = sample_permission_modal();
    modal.selected = 2;
    let theme = Theme::default();
    let lines = render_modal_options(
        &modal,
        ModalOptionsLayout {
            inner_width: 14,
            max_visible: 1,
            theme: &theme,
        },
    );
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert!(line.contains("Always"));
    assert!(line.contains('‹') || line.contains('›'));
}

#[test]
fn test_horizontal_modal_hints_for_select_and_input_modes() {
    let mut modal = sample_permission_modal();
    assert_eq!(
        modal_hint(&modal),
        "←/→ or h/l select • ↑/↓ or j/k scroll • Enter confirm • Esc deny"
    );
    modal.mode = ModalMode::Input {
        prompt_label: "cmd".into(),
    };
    assert_eq!(modal_hint(&modal), "Enter submit • Esc back");
}
