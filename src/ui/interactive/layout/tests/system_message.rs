use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{Activity, EditorState, FooterState, InteractiveLayout};

fn layout_with(system_message: Option<&str>, footer: &FooterState) -> InteractiveLayout {
    let editor = EditorState::default();
    layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer,
        system_message,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
    })
}

#[test]
fn system_message_renders_in_footer_status_slot() {
    let footer = FooterState::default();
    let layout = layout_with(Some("Model: claude-3-5-sonnet (anthropic)"), &footer);
    assert_eq!(layout.footer_lines.len(), 2);
    let top = &layout.footer_lines[0];
    assert!(top.contains("Model: claude-3-5-sonnet"));
    assert!(layout.lines.iter().any(|l| l.contains("claude-3-5-sonnet")));
}

#[test]
fn system_message_does_not_change_layout_height_or_cursor_row() {
    let footer = FooterState::default();
    let idle = layout_with(None, &footer);
    let notice = layout_with(Some("Model: gpt-4o (openai)"), &footer);
    assert_eq!(notice.height(), idle.height());
    assert_eq!(notice.cursor_row(), idle.cursor_row());
    assert_eq!(notice.editor_lines, idle.editor_lines);
    assert_eq!(notice.footer_lines.len(), idle.footer_lines.len());
}

#[test]
fn expired_message_falls_back_to_quota_in_status_slot() {
    let footer = FooterState {
        activity: Activity::Idle,
        cwd: Some("/work".into()),
        quota: Some("5h: 80%".into()),
        ..FooterState::default()
    };
    let active = layout_with(Some("Model: gpt-4o (openai)"), &footer);
    assert!(active.footer_lines[0].contains("Model: gpt-4o"));
    assert!(!active.footer_lines[0].contains("5h: 80%"));

    let expired = layout_with(None, &footer);
    assert!(expired.footer_lines[0].ends_with("5h: 80%"));
}

#[test]
fn multiline_message_flattens_without_growing_layout() {
    let footer = FooterState::default();
    let layout = layout_with(Some("Steering queued\nat tool boundary"), &footer);
    assert!(layout.footer_lines[0].contains("Steering queued at tool boundary"));
    assert_eq!(layout.footer_lines.len(), 2);
    assert_eq!(layout.height(), layout_with(None, &footer).height());
}

#[test]
fn blank_message_falls_back_to_persistent_status() {
    let footer = FooterState {
        activity: Activity::Idle,
        quota: Some("5h: 80%".into()),
        ..FooterState::default()
    };
    let layout = layout_with(Some("   "), &footer);
    assert!(layout.footer_lines[0].ends_with("5h: 80%"));
    assert!(!layout.footer_lines[0].contains("Model:"));
}

#[test]
fn spinner_and_message_coexist_without_growing_layout() {
    let working = FooterState {
        activity: Activity::Working,
        ..FooterState::default()
    };
    let with_message = layout_with(Some("Tool output: collapsed"), &working);
    let without_message = layout_with(None, &working);
    assert!(!with_message.working_line.is_empty());
    assert!(with_message.footer_lines[0].contains("Tool output: collapsed"));
    assert_eq!(with_message.height(), without_message.height());
}
