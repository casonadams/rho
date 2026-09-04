use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState, ModalOption, ModalState};

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
        theme: None,
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
        theme: None,
    });

    assert!(layout.lines.len() <= 15);
    assert!(layout.cursor_visible);
    assert_eq!(layout.cursor_row, 2);
    assert!(layout.cursor_row < layout.lines.len());
    assert!(layout.cursor.column <= 80);
}
