use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState, ModalOption, ModalState};

#[test]
fn modal_hides_widget_lines() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let modal = ModalState::new("Permission Required", "tool bash", vec![ModalOption::from("Allow")]);
    let widgets = vec!["● Todos (1/2)".to_string()];
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: Some(&modal),
        autocomplete: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &widgets,
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert!(layout.widget_lines.is_empty());
}

#[test]
fn modal_layout_renders_input_frame_style() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let modal = ModalState::new(
        "Permission Required",
        "tool   bash\nscope  cargo test",
        vec![ModalOption::from("Allow"), ModalOption::from("Deny with reason")],
    );
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: Some(&modal),
        autocomplete: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 40,
        spinner_frame: 0,
    });

    assert!(layout.top_divider.is_empty());
    assert!(layout.editor_lines.iter().any(|l| l.contains("─".repeat(40).as_str())));
    assert!(layout.editor_lines.iter().any(|l| l.contains("Permission Required")));
    assert!(layout.editor_lines.iter().any(|l| l.contains("tool   bash")));
    assert!(layout.editor_lines.iter().any(|l| l.contains("Allow")));
    assert!(!layout.cursor_visible);
}

#[test]
fn searchable_modal_renders_with_unified_header_and_indicator() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let modal = ModalState::new(
        "Select Model",
        "",
        vec![
            ModalOption::new("model-a", Some("openai\t✓\tdefault\t128k ctx")),
            ModalOption::new("model-b", Some("anthropic\t\t\t200k ctx")),
        ],
    )
    .with_search(true);

    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: Some(&modal),
        autocomplete: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 50,
        spinner_frame: 0,
    });

    assert!(layout.editor_lines.iter().any(|l| l.contains("Select Model")));
    assert!(layout.editor_lines.iter().any(|l| l.contains(">")));
    assert!(layout.editor_lines.iter().any(|l| l.contains("▸")));
    assert!(layout.editor_lines.iter().any(|l| l.contains("model-a")));
    assert!(layout.editor_lines.iter().any(|l| l.contains("[openai]")));
    assert!(layout.cursor_visible);
}
