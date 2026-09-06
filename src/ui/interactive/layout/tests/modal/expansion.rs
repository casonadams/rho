use crate::ui::interactive::layout::modal::in_input::{calculate_content_space, in_input_modal_desired_lines};
use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState, ModalOption, ModalState, OptionLayout};

#[test]
fn test_in_input_modal_desired_lines_uncapped_body() {
    let body = (1..=20).map(|i| format!("cmd {i}")).collect::<Vec<_>>().join("\n");
    let mut modal = ModalState::new("Run", &body, vec![ModalOption::from("Allow")]);
    modal.option_layout = OptionLayout::Horizontal;
    let desired = in_input_modal_desired_lines(&modal, "", 80);
    assert_eq!(desired, 21);
}

#[test]
fn test_calculate_content_space_horizontal_layout() {
    let mut modal = ModalState::new("Run", "body", vec![ModalOption::from("Allow")]);
    modal.option_layout = OptionLayout::Horizontal;
    assert_eq!(calculate_content_space(&modal, 0), (0, 0));
    assert_eq!(calculate_content_space(&modal, 1), (0, 1));
    assert_eq!(calculate_content_space(&modal, 5), (4, 1));
    assert_eq!(calculate_content_space(&modal, 15), (14, 1));
}

#[test]
fn test_calculate_content_space_vertical_layout() {
    let modal = ModalState::new(
        "Run",
        "body",
        vec![ModalOption::from("A"), ModalOption::from("B"), ModalOption::from("C")],
    );
    assert_eq!(calculate_content_space(&modal, 8), (5, 3));
}

fn build_layout_with_modal(
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
fn test_modal_expands_beyond_8_lines_on_tall_terminal() {
    let body = (1..=25).map(|i| format!("arg_line_{i}")).collect::<Vec<_>>().join("\n");
    let mut modal = ModalState::new("Perm", &body, vec![ModalOption::from("Allow")]);
    modal.option_layout = OptionLayout::Horizontal;
    let rendered = build_layout_with_modal(&modal, 40);
    let visible_body = rendered.lines.iter().filter(|l| l.contains("arg_line_")).count();
    assert!(visible_body > 8);
    assert!(rendered.lines.len() <= 40);
}

#[test]
fn test_modal_bottom_pinning_invariant() {
    for height in [15, 25, 40] {
        let body = (1..=20).map(|i| format!("arg_{i}")).collect::<Vec<_>>().join("\n");
        let mut modal = ModalState::new("Perm", &body, vec![ModalOption::from("Allow")]);
        modal.option_layout = OptionLayout::Horizontal;
        let rendered = build_layout_with_modal(&modal, height);
        let bot_div_idx = rendered.lines.iter().rposition(|l| l.contains("────────")).unwrap();
        assert_eq!(bot_div_idx, rendered.lines.len() - 2);
    }
}
