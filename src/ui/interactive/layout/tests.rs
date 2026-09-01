use super::{CursorPosition, LayoutInput, layout};
use crate::ui::interactive::{Activity, EditorState, FooterState};
use unicode_width::UnicodeWidthStr;

#[test]
fn widget_lines_affect_height_and_cursor_row() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let widgets = vec![
        "● Todos (1/2)".to_string(),
        "├─ ✓ #1 Done".to_string(),
        "└─ ○ #2 Pending".to_string(),
    ];
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &widgets,
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert_eq!(layout.widget_lines.len(), 3);
    // 1 (editor) + 0 (queued) + 2 (footer) + 0 (working) + 1 (top_divider) + 1 (bottom_divider) + 3 (widgets) + 1 (spacer) = 9
    assert_eq!(layout.height(), 9);
    // cursor_row: 0 (queued) + 4 (widgets + spacer) + 0 (working) + 1 (top_divider) + 0 (cursor.row) = 5
    assert_eq!(layout.cursor_row(), 5);
}

#[test]
fn modal_hides_widget_lines() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let modal = crate::ui::interactive::ModalState::new(
        "Permission Required",
        "tool bash",
        vec![crate::ui::interactive::ModalOption::from("Allow")],
    );
    let widgets = vec!["● Todos (1/2)".to_string()];
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: Some(&modal),
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &widgets,
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert!(layout.widget_lines.is_empty());
}

#[test]
fn empty_editor_has_one_line_and_fixed_chrome() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 8,
        spinner_frame: 0,
    });

    assert_eq!(layout.top_divider, "\u{1b}[2m────────\u{1b}[0m");
    assert_eq!(layout.editor_lines, [""]);
    assert_eq!(layout.footer_lines.len(), 2);
    assert_eq!(layout.cursor, CursorPosition { row: 0, column: 0 });
    assert_eq!(layout.height(), 5);
}

#[test]
fn explicit_newlines_grow_the_editor() {
    let mut editor = EditorState::default();
    editor.set_text("one\ntwo\n");
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 20,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines, ["one", "two", ""]);
    assert_eq!(layout.cursor, CursorPosition { row: 2, column: 0 });
    assert_eq!(layout.height(), 7);
}

#[test]
fn soft_wrap_uses_display_width_for_wide_unicode() {
    let mut editor = EditorState::default();
    editor.set_text("ab界c");
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 4,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines, ["ab界", "c"]);
    assert_eq!(layout.cursor, CursorPosition { row: 1, column: 1 });
}

#[test]
fn cursor_tracks_insertion_position_across_wrapped_lines() {
    let mut editor = EditorState::default();
    editor.set_text("abcdef");
    editor.move_left();
    editor.move_left();
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 3,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines, ["abc", "def"]);
    assert_eq!(layout.cursor, CursorPosition { row: 1, column: 1 });
}

#[test]
fn full_final_line_adds_a_cursor_line() {
    let mut editor = EditorState::default();
    editor.set_text("界");
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 2,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines, ["界", ""]);
    assert_eq!(layout.cursor, CursorPosition { row: 1, column: 0 });
}

#[test]
fn footer_contains_available_status_and_queue_count() {
    let default_editor = EditorState::default();
    let footer = FooterState {
        activity: Activity::Thinking,
        model: "model".into(),
        context: Some("42% context".into()),
        quota: Some("80% quota".into()),
        ..FooterState::default()
    };
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        footer: &footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert!(layout.footer_lines[0].ends_with("80% quota"));
    assert!(layout.footer_lines[1].ends_with("model"));
}

#[test]
fn queued_messages_render_above_the_working_line() {
    let default_editor = EditorState::default();
    let footer = FooterState {
        activity: Activity::Working,
        model: "model".into(),
        context: None,
        quota: None,
        ..FooterState::default()
    };
    let queued = vec![
        crate::ui::interactive::QueuedMessage {
            text: "first steer".into(),
            kind: crate::ui::interactive::QueueKind::Steering,
        },
        crate::ui::interactive::QueuedMessage {
            text: "next follow".into(),
            kind: crate::ui::interactive::QueueKind::FollowUp,
        },
    ];
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        footer: &footer,
        queued_messages: &queued,
        widget_lines: &[],
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert_eq!(layout.queued_lines.len(), 3);
    assert!(layout.queued_lines[0].contains("Steering: first steer"));
    assert!(layout.queued_lines[1].contains("Follow-up: next follow"));
    assert!(layout.queued_lines[2].contains("Alt+↑"));
    assert_eq!(layout.height(), 10);
}

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
        footer: &footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert!(layout.working_line.contains('\u{280b}'));
    assert!(layout.working_line.contains("Working..."));
    assert!(layout.working_line.contains("\u{1b}[2m"));
    assert!(layout.footer_lines[1].ends_with("model"));
    assert_eq!(layout.height(), 7);
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
        footer: &footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert!(layout.working_line.contains('\u{280b}'));
    assert!(layout.working_line.contains("Working..."));
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
        footer: &footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert_eq!(layout.working_line, "");
    assert_eq!(layout.height(), 5);
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
    let modal = crate::ui::interactive::ModalState::new(
        "Permission Required",
        "tool   bash\nscope  cargo test",
        vec![crate::ui::interactive::ModalOption::from("Allow")],
    );
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: Some(&modal),
        footer: &footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert_eq!(layout.working_line, "");
}

#[test]
fn editor_layout_tracks_lines_and_dividers() {
    let mut editor = EditorState::default();
    editor.set_text("draft");
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines.len(), 1);
    assert_eq!(layout.height(), 5);
    assert_eq!(layout.cursor_row(), 1);
}

#[test]
fn multiline_editor_height_matches_content() {
    let mut editor = EditorState::default();
    editor.set_text("line1\nline2\nline3");
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines.len(), 3);
    assert_eq!(layout.height(), 7);
}

#[test]
fn narrow_layout_never_exceeds_terminal_width() {
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
        footer: &footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 5,
        spinner_frame: 1,
    });

    assert!(layout.footer_lines[0].width() <= 5);
    assert!(layout.footer_lines[1].width() <= 5);
    assert_eq!(
        crate::ui::interactive::layout::text::visible_width(&layout.top_divider),
        5
    );
}

#[test]
fn modal_layout_renders_input_frame_style() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let modal = crate::ui::interactive::ModalState::new(
        "Permission Required",
        "tool   bash\nscope  cargo test",
        vec![
            crate::ui::interactive::ModalOption::from("Allow"),
            crate::ui::interactive::ModalOption::from("Deny with reason"),
        ],
    );
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: Some(&modal),
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
    assert_eq!(layout.cursor.column, 2);
}

#[test]
fn truncate_to_visual_lines_preserves_short_content() {
    let text = "line1\nline2\nline3";
    let res = super::truncate_to_visual_lines(text, 5, 40);
    assert_eq!(res.visual_lines, ["line1", "line2", "line3"]);
    assert_eq!(res.skipped_count, 0);
}

#[test]
fn truncate_to_visual_lines_skips_earlier_lines_when_exceeding_limit() {
    let text = "line1\nline2\nline3\nline4\nline5\nline6\nline7";
    let res = super::truncate_to_visual_lines(text, 5, 40);
    assert_eq!(res.visual_lines, ["line3", "line4", "line5", "line6", "line7"]);
    assert_eq!(res.skipped_count, 2);
}

#[test]
fn thinking_borders_change_color_with_thinking_level() {
    let default_editor = EditorState::default();
    let levels = [
        (None, "\u{1b}[2m"),
        (Some("off"), "\u{1b}[2m"),
        (Some("minimal"), "\u{1b}[90m"),
        (Some("low"), "\u{1b}[34m"),
        (Some("medium"), "\u{1b}[36m"),
        (Some("high"), "\u{1b}[35m"),
        (Some("xhigh"), "\u{1b}[95m"),
        (Some("max"), "\u{1b}[1;95m"),
    ];

    for (level, expected_style) in levels {
        let footer = FooterState {
            thinking_level: level.map(ToString::to_string),
            ..FooterState::default()
        };
        let layout = layout(LayoutInput {
            editor: &default_editor,
            modal: None,
            footer: &footer,
            queued_messages: &[],
            widget_lines: &[],
            terminal_width: 10,
            spinner_frame: 0,
        });

        assert!(
            layout.top_divider.starts_with(expected_style),
            "level {:?} expected style {:?}, got {:?}",
            level,
            expected_style,
            layout.top_divider
        );
        assert!(layout.bottom_divider.starts_with(expected_style));
    }
}
