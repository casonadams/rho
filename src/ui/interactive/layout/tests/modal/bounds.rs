use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState, ModalOption, ModalState};

#[test]
fn modal_body_truncation_on_small_terminal() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let body = (1..=30)
        .map(|i| format!("command argument line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let modal = ModalState::new(
        "Permission Required",
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
    });

    assert!(layout.lines.len() <= 15);
    let allow_idx = layout
        .lines
        .iter()
        .position(|l| l.contains("Allow"))
        .expect("Allow visible");
    let deny_idx = layout
        .lines
        .iter()
        .position(|l| l.contains("Deny"))
        .expect("Deny visible");
    assert!(allow_idx < deny_idx);

    let omitted_idx = layout
        .lines
        .iter()
        .position(|l| l.contains("lines omitted"))
        .expect("truncation indicator visible");
    assert!(omitted_idx < allow_idx);

    let rendered_body_lines = layout
        .lines
        .iter()
        .filter(|l| l.contains("command argument line"))
        .count();
    let omitted_count: usize = layout.lines[omitted_idx]
        .split("[... ")
        .nth(1)
        .and_then(|s| s.split(" lines omitted").next())
        .expect("extract count")
        .parse()
        .expect("parse count");
    assert_eq!(rendered_body_lines + omitted_count, 30);
    assert!(
        layout
            .lines
            .last()
            .expect("bottom line")
            .contains("─".repeat(80).as_str())
    );
}

#[test]
fn modal_body_truncation_minimal_omitted_lines() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let body = (1..=7).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
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
    });

    assert!(layout.lines.len() <= 15);
    assert!(layout.lines.iter().any(|l| l.contains("2 lines omitted")));
}

#[test]
fn modal_body_suppressed_on_minimal_terminal_height() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let body = (1..=30)
        .map(|i| format!("command argument line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let modal = ModalState::new(
        "Permission Required",
        &body,
        vec![ModalOption::from("Allow"), ModalOption::from("Deny")],
    );

    let layout_8 = layout(LayoutInput {
        editor: &default_editor,
        modal: Some(&modal),
        autocomplete: None,
        footer: &default_footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 8,
        spinner_frame: 0,
    });
    assert!(layout_8.lines.len() <= 8);
    assert!(layout_8.lines.iter().any(|l| l.contains("Allow")));
    assert!(layout_8.lines.iter().any(|l| l.contains("Deny")));
    assert!(!layout_8.lines.iter().any(|l| l.contains("command argument line")));

    let layout_6 = layout(LayoutInput {
        editor: &default_editor,
        modal: Some(&modal),
        autocomplete: None,
        footer: &default_footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 6,
        spinner_frame: 0,
    });
    assert!(layout_6.lines.len() <= 6);
    assert!(layout_6.lines.iter().any(|l| l.contains("Allow")));
    assert!(
        layout_6
            .lines
            .last()
            .expect("bottom line")
            .contains("─".repeat(80).as_str())
    );
}

#[test]
fn modal_input_mode_cursor_with_body_truncation() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let body = (1..=30)
        .map(|i| format!("long description line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut modal = ModalState::new("Reason Required", &body, vec![ModalOption::from("Confirm")]);
    modal.mode = crate::ui::interactive::ModalMode::Input {
        prompt_label: "Reason".to_string(),
    };
    modal.input.set_text("test reason");

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
    });

    assert!(layout.lines.len() <= 15);
    assert!(layout.cursor_visible);
    assert!(layout.cursor_row < layout.lines.len());
    assert!(layout.cursor.column <= 80);
    assert!(layout.lines[layout.cursor_row].contains("Reason:"));
}

#[test]
fn modal_searchable_cursor_with_body_truncation() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let body = (1..=30)
        .map(|i| format!("model detail line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let modal = ModalState::new("Select Model", &body, vec![ModalOption::from("model-1")]).with_search(true);

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
    });

    assert!(layout.lines.len() <= 15);
    assert!(layout.cursor_visible);
    assert_eq!(layout.cursor_row, 2);
    assert!(layout.cursor_row < layout.lines.len());
    assert!(layout.cursor.column <= 80);
}
