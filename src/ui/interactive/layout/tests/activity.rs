use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{Activity, EditorState, FooterState, ModalOption, ModalState};

#[test]
fn busy_activity_renders_working_line_above_the_editor() {
    let default_editor = EditorState::default();
    let footer = FooterState {
        activity: Activity::Working,
        model: "model".into(),
        context: None,
        quota: None,
        ..FooterState::default()
    };
    let layout = layout(LayoutInput {
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
        theme: None,
    });

    assert!(layout.working_line.contains('\u{280b}'));
    assert!(layout.working_line.contains("Working..."));
    assert!(layout.working_line.contains("\u{1b}[2m"));
    assert!(layout.footer_lines[1].ends_with("model"));
    assert_eq!(layout.height(), 6);
}

#[test]
fn thinking_activity_also_renders_the_working_line() {
    let default_editor = EditorState::default();
    let footer = FooterState {
        activity: Activity::Thinking,
        model: "model".into(),
        context: None,
        quota: None,
        ..FooterState::default()
    };
    let layout = layout(LayoutInput {
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
        theme: None,
    });

    assert!(layout.working_line.contains('\u{280b}'));
    assert!(layout.working_line.contains("Working..."));
}

#[test]
fn compacting_activity_renders_compacting_label() {
    let default_editor = EditorState::default();
    let footer = FooterState {
        activity: Activity::Compacting,
        model: "model".into(),
        context: None,
        quota: None,
        ..FooterState::default()
    };
    let layout = layout(LayoutInput {
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
        theme: None,
    });

    assert!(layout.working_line.contains('\u{280b}'));
    assert!(layout.working_line.contains("Compacting..."));
    assert!(!layout.working_line.contains("Working..."));
}

#[test]
fn idle_activity_renders_no_working_line() {
    let default_editor = EditorState::default();
    let footer = FooterState {
        activity: Activity::Idle,
        model: "model".into(),
        context: None,
        quota: None,
        ..FooterState::default()
    };
    let layout = layout(LayoutInput {
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
        theme: None,
    });

    assert_eq!(layout.working_line, "");
}

#[test]
fn busy_activity_does_not_change_layout_height_or_cursor_row() {
    let default_editor = EditorState::default();
    let idle = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        autocomplete: None,
        footer: &FooterState::default(),
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
    });
    let busy_footer = FooterState {
        activity: Activity::Working,
        ..FooterState::default()
    };
    let busy = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        autocomplete: None,
        footer: &busy_footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
    });

    assert_eq!(busy.height(), idle.height());
    assert_eq!(busy.cursor_row(), idle.cursor_row());
    assert_eq!(busy.editor_lines, idle.editor_lines);
    assert!(!busy.working_line.is_empty());
}

#[test]
fn busy_activity_under_modal_hides_working_line() {
    let default_editor = EditorState::default();
    let footer = FooterState {
        activity: Activity::Working,
        model: "model".into(),
        context: None,
        quota: None,
        ..FooterState::default()
    };
    let modal = ModalState::new(
        "Permission Required",
        "tool   bash\nscope  cargo test",
        vec![ModalOption::from("Allow")],
    );
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: Some(&modal),
        autocomplete: None,
        footer: &footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
    });

    assert_eq!(layout.working_line, "");
}
