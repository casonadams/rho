use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState, ModalOption, ModalState};

fn modal_cursor_layout(modal: &ModalState) -> crate::ui::interactive::layout::InteractiveLayout {
    layout(LayoutInput {
        editor: &EditorState::default(),
        modal: Some(modal),
        autocomplete: None,
        footer: &FooterState::default(),
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 15,
        spinner_frame: 0,
        theme: None,
    })
}

fn assert_modal_cursor_valid(layout: &crate::ui::interactive::layout::InteractiveLayout, expected_content: &str) {
    assert!(layout.lines.len() <= 15 && layout.cursor_visible);
    assert!(layout.cursor_row < layout.lines.len() && layout.cursor.column <= 80);
    assert!(layout.lines[layout.cursor_row].contains(expected_content));
}

#[test]
fn modal_input_mode_cursor_with_body_truncation() {
    let body = (1..=30)
        .map(|i| format!("long description line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut modal = ModalState::new("Reason Required", &body, vec![ModalOption::from("Confirm")]);
    modal.mode = crate::ui::interactive::ModalMode::Input {
        prompt_label: "Reason".to_string(),
    };
    modal.input.set_text("test reason");

    let layout = modal_cursor_layout(&modal);
    assert_modal_cursor_valid(&layout, "test reason");
    assert!(layout.top_divider.contains("Reason"));
}

#[test]
fn modal_searchable_cursor_with_body_truncation() {
    let body = (1..=30)
        .map(|i| format!("model detail line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let modal = ModalState::new("Select Model", &body, vec![ModalOption::from("model-1")]).with_search(true);

    let layout = modal_cursor_layout(&modal);
    assert_eq!(layout.cursor_row, 1);
    assert_modal_cursor_valid(&layout, ">");
}
