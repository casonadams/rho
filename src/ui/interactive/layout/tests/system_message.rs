use crate::ui::interactive::layout::{LayoutInput, layout, system_lines_text};
use crate::ui::interactive::{Activity, EditorState, FooterState};

#[test]
fn system_message_renders_in_dedicated_area_above_editor() {
    let default_editor = EditorState::default();
    let footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        autocomplete: None,
        footer: &footer,
        system_message: Some("Model: claude-3-5-sonnet (anthropic)"),
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
    });

    assert_eq!(layout.system_lines.len(), 1);
    assert!(layout.system_lines[0].contains("Model: claude-3-5-sonnet"));
    assert!(layout.system_lines[0].contains('ℹ'));

    let sys_pos = layout
        .lines
        .iter()
        .position(|l| l.contains("Model: claude-3-5-sonnet"))
        .unwrap();
    let top_div_pos = layout.lines.iter().position(|l| l == &layout.top_divider).unwrap();
    assert!(sys_pos < top_div_pos);
    assert_eq!(layout.height(), 7);
}

#[test]
fn system_message_collapses_when_empty_or_none() {
    let default_editor = EditorState::default();
    let footer = FooterState::default();
    let layout_none = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        autocomplete: None,
        footer: &footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
    });
    assert!(layout_none.system_lines.is_empty());
    assert_eq!(layout_none.height(), 6);

    let layout_empty = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        autocomplete: None,
        footer: &footer,
        system_message: Some("   "),
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
    });
    assert!(layout_empty.system_lines.is_empty());
    assert_eq!(layout_empty.height(), 6);
}

#[test]
fn system_message_and_spinner_render_together_in_dedicated_area() {
    let default_editor = EditorState::default();
    let footer = FooterState {
        activity: Activity::Working,
        ..FooterState::default()
    };
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        autocomplete: None,
        footer: &footer,
        system_message: Some("Tool output: collapsed"),
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
    });

    assert_eq!(layout.system_lines.len(), 1);
    assert!(!layout.working_line.is_empty());

    let sys_pos = layout
        .lines
        .iter()
        .position(|l| l.contains("Tool output: collapsed"))
        .unwrap();
    let spinner_pos = layout.lines.iter().position(|l| l.contains("Working...")).unwrap();
    let top_div_pos = layout.lines.iter().position(|l| l == &layout.top_divider).unwrap();

    assert!(sys_pos < spinner_pos);
    assert!(spinner_pos < top_div_pos);
    assert_eq!(layout.height(), 8);
}

#[test]
fn system_lines_text_respects_narrow_widths() {
    assert!(system_lines_text(Some("Hello"), 4).is_empty());
    let res = system_lines_text(Some("Hello"), 10);
    assert_eq!(res.len(), 1);
}
