use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState, ModalOption, ModalState, OptionLayout};

fn sample_scrollable_modal(body_lines: usize, option_layout: OptionLayout) -> ModalState {
    let body = (1..=body_lines)
        .map(|i| format!("command_arg_line_{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut modal = ModalState::new(
        "Permission Required",
        &body,
        vec![
            ModalOption::from("Allow"),
            ModalOption::from("Edit"),
            ModalOption::from("Always"),
            ModalOption::from("Deny"),
        ],
    );
    modal.option_layout = option_layout;
    modal
}

fn render_modal_layout(
    modal: &ModalState,
    terminal_height: usize,
) -> crate::ui::interactive::layout::InteractiveLayout {
    let ed = EditorState::default();
    let ft = FooterState::default();
    layout(LayoutInput {
        editor: &ed,
        modal: Some(modal),
        autocomplete: None,
        footer: &ft,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height,
        spinner_frame: 0,
        theme: None,
    })
}

#[test]
fn test_horizontal_modal_body_scroll_indicator_initial() {
    let modal = sample_scrollable_modal(30, OptionLayout::Horizontal);
    let rendered = render_modal_layout(&modal, 15);
    assert!(rendered.lines.iter().any(|l| l.contains("↑/↓ scroll (line 1/30)")));
    assert!(rendered.lines.iter().any(|l| l.trim() == "command_arg_line_1"));
}

#[test]
fn test_horizontal_modal_body_scroll_offset() {
    let mut modal = sample_scrollable_modal(30, OptionLayout::Horizontal);
    modal.body_scroll = 5;
    let rendered = render_modal_layout(&modal, 15);
    assert!(rendered.lines.iter().any(|l| l.contains("↑/↓ scroll (line 6/30)")));
    assert!(!rendered.lines.iter().any(|l| l.trim() == "command_arg_line_1"));
    assert!(rendered.lines.iter().any(|l| l.trim() == "command_arg_line_6"));
}

#[test]
fn test_horizontal_modal_body_scroll_clamped_at_end() {
    let mut modal = sample_scrollable_modal(30, OptionLayout::Horizontal);
    modal.body_scroll = 500;
    let rendered = render_modal_layout(&modal, 15);
    assert!(rendered.lines.iter().any(|l| l.contains("↑/↓ scroll (line ")));
    assert!(rendered.lines.iter().any(|l| l.trim() == "command_arg_line_30"));
}

#[test]
fn test_horizontal_modal_body_fits_no_scroll_indicator() {
    let modal = sample_scrollable_modal(3, OptionLayout::Horizontal);
    let rendered = render_modal_layout(&modal, 20);
    assert!(!rendered.lines.iter().any(|l| l.contains("↑/↓ scroll")));
    assert!(!rendered.lines.iter().any(|l| l.contains("lines omitted")));
    for i in 1..=3 {
        assert!(
            rendered
                .lines
                .iter()
                .any(|l| l.trim() == format!("command_arg_line_{i}"))
        );
    }
}

#[test]
fn test_vertical_modal_retains_omitted_indicator() {
    let modal = sample_scrollable_modal(30, OptionLayout::Vertical);
    let rendered = render_modal_layout(&modal, 15);
    assert!(rendered.lines.iter().any(|l| l.contains("lines omitted")));
    assert!(!rendered.lines.iter().any(|l| l.contains("↑/↓ scroll")));
}
