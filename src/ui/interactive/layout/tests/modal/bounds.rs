use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState, ModalOption, ModalState};

fn sample_permission_modal(n: usize) -> ModalState {
    let body = (1..=n)
        .map(|i| format!("command argument line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    ModalState::new(
        "Permission Required",
        &body,
        vec![ModalOption::from("Allow"), ModalOption::from("Deny")],
    )
}

fn modal_test_layout(modal: &ModalState, terminal_height: usize) -> crate::ui::interactive::layout::InteractiveLayout {
    layout(LayoutInput {
        editor: &EditorState::default(),
        modal: Some(modal),
        autocomplete: None,
        footer: &FooterState::default(),
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height,
        spinner_frame: 0,
        theme: None,
    })
}

fn parse_omitted_count(line: &str) -> usize {
    line.split("[... ")
        .nth(1)
        .and_then(|s| s.split(" lines omitted").next())
        .expect("extract count")
        .parse()
        .expect("parse count")
}

#[test]
fn modal_body_truncation_on_small_terminal() {
    let modal = sample_permission_modal(30);
    let layout = modal_test_layout(&modal, 15);

    assert!(layout.lines.len() <= 15);
    let allow_idx = layout.lines.iter().position(|l| l.contains("Allow")).unwrap();
    let deny_idx = layout.lines.iter().position(|l| l.contains("Deny")).unwrap();
    let omitted_idx = layout.lines.iter().position(|l| l.contains("lines omitted")).unwrap();
    assert!(omitted_idx < allow_idx && allow_idx < deny_idx);

    let rendered_lines = layout
        .lines
        .iter()
        .filter(|l| l.contains("command argument line"))
        .count();
    let omitted_count = parse_omitted_count(&layout.lines[omitted_idx]);
    assert_eq!(rendered_lines + omitted_count, 30);
    assert!(layout.bottom_divider.contains("─".repeat(80).as_str()));
}

#[test]
fn modal_body_truncation_minimal_omitted_lines() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let body = (1..=9).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let modal = ModalState::new(
        "Permission",
        &body,
        vec![ModalOption::from("Allow"), ModalOption::from("Deny")],
    );

    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: Some(&modal),
        autocomplete: None,
        footer: &default_footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 15,
        spinner_frame: 0,
        theme: None,
    });

    assert!(layout.lines.len() <= 15);
    assert!(layout.lines.iter().any(|l| l.contains("2 lines omitted")));
}

#[test]
fn modal_body_fits_minimal_on_terminal_height_8() {
    let modal = sample_permission_modal(30);
    let layout_8 = modal_test_layout(&modal, 8);
    assert!(layout_8.lines.len() <= 8);
    assert!(layout_8.lines.iter().any(|l| l.contains("Allow")));
    assert!(layout_8.lines.iter().any(|l| l.contains("Deny")));
}

#[test]
fn modal_body_suppressed_on_minimal_terminal_height_6() {
    let modal = sample_permission_modal(30);
    let layout_6 = modal_test_layout(&modal, 6);
    assert!(layout_6.lines.len() <= 6);
    assert!(layout_6.lines.iter().any(|l| l.contains("Allow")));
    assert!(layout_6.bottom_divider.contains("─".repeat(80).as_str()));
    assert!(!layout_6.lines.iter().any(|l| l.contains("command argument line")));
}
