use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState, ModalOption, ModalState};

#[test]
fn modal_preserves_widget_lines_when_budget_permits() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let modal = ModalState::new("Permission Required", "tool bash", vec![ModalOption::from("Allow")]);
    let widgets = vec!["● Todos (1/2)".to_string()];
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: Some(&modal),
        autocomplete: None,
        footer: &default_footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &widgets,
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
    });

    assert_eq!(layout.widget_lines, widgets);
}

fn render_test_modal(modal: &ModalState, width: usize) -> crate::ui::interactive::layout::InteractiveLayout {
    layout(LayoutInput {
        editor: &EditorState::default(),
        modal: Some(modal),
        autocomplete: None,
        footer: &FooterState::default(),
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: width,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
    })
}

#[test]
fn modal_layout_renders_input_frame_style() {
    let modal = ModalState::new(
        "Permission Required",
        "tool   bash\nscope  cargo test",
        vec![ModalOption::from("Allow"), ModalOption::from("Deny with reason")],
    );
    let layout = render_test_modal(&modal, 40);

    assert!(layout.top_divider.contains("Permission Required") && !layout.cursor_visible);
    assert!(layout.bottom_divider.contains("─".repeat(40).as_str()));
    assert!(layout.editor_lines.iter().any(|l| l.contains("tool   bash")));
    assert!(layout.editor_lines.iter().any(|l| l.contains("Allow")));
}

#[test]
fn searchable_modal_renders_with_unified_header_and_indicator() {
    let modal = ModalState::new(
        "Select Model",
        "",
        vec![
            ModalOption::new("model-a", Some("openai\t✓\tdefault\t128k ctx")),
            ModalOption::new("model-b", Some("anthropic\t\t\t200k ctx")),
        ],
    )
    .with_search(true);

    let layout = render_test_modal(&modal, 50);
    assert!(layout.top_divider.contains("Select Model") && layout.cursor_visible);
    for token in ['>', '▸'] {
        assert!(layout.editor_lines.iter().any(|l| l.contains(token)));
    }
    assert!(
        layout
            .editor_lines
            .iter()
            .any(|l| l.contains("model-a") && l.contains("[openai]"))
    );
}

#[test]
fn modal_renders_docked_draft_when_editor_contains_text() {
    let mut editor = EditorState::default();
    editor.set_text("draft message to preserve");
    let default_footer = FooterState::default();
    let modal = ModalState::new("Select Model", "", vec![ModalOption::from("model-a")]);

    let layout = layout(LayoutInput {
        editor: &editor,
        modal: Some(&modal),
        autocomplete: None,
        footer: &default_footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
    });

    assert!(layout.top_divider.contains("Select Model"));
    assert!(
        layout
            .editor_lines
            .iter()
            .any(|l| l.contains("Draft: \"draft message to preserve\" (restores on close)"))
    );
    assert_eq!(editor.text(), "draft message to preserve");
}

#[test]
fn modal_without_draft_omits_docked_draft_line() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let modal = ModalState::new("Select Model", "", vec![ModalOption::from("model-a")]);

    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: Some(&modal),
        autocomplete: None,
        footer: &default_footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
    });

    assert!(!layout.editor_lines.iter().any(|l| l.contains("Draft:")));
}
