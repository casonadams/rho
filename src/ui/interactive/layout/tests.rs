use super::{CursorPosition, LayoutInput, layout};
use crate::ui::interactive::{Activity, EditorState, FooterState};
use unicode_width::UnicodeWidthStr;

#[test]
fn empty_editor_has_one_line_and_fixed_chrome() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        footer: &default_footer,
        queued_messages: &[],
        terminal_width: 8,
        spinner_frame: 0,
    });

    assert_eq!(layout.top_divider, "────────");
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
        terminal_width: 5,
        spinner_frame: 1,
    });

    assert!(layout.footer_lines[0].width() <= 5);
    assert!(layout.footer_lines[1].width() <= 5);
    assert_eq!(layout.top_divider.width(), 5);
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
fn format_active_tool_block_contains_command_and_elapsed() {
    let theme = crate::ui::theme::Theme::default();
    let formatted = super::format_active_tool_block(super::ActiveToolDisplayInput {
        tool_name: "bash",
        args_summary: "cargo test",
        preview: None,
        output: "compiling...\ntest result: ok",
        started: std::time::Instant::now(),
        theme: &theme,
        width: 60,
        expanded: false,
    });

    assert!(formatted.contains("bash"));
    assert!(formatted.contains("cargo test"));
    assert!(formatted.contains("test result: ok"));
    assert!(formatted.contains("Elapsed"));
}

#[test]
fn format_active_tool_block_renders_diff_preview_for_edit() {
    let theme = crate::ui::theme::Theme::default();
    let diff_preview = "```diff\n- old text\n+ new text\n```";
    let formatted = super::format_active_tool_block(super::ActiveToolDisplayInput {
        tool_name: "edit",
        args_summary: "src/main.rs (1 edits)",
        preview: Some(diff_preview),
        output: "",
        started: std::time::Instant::now(),
        theme: &theme,
        width: 60,
        expanded: false,
    });

    assert!(formatted.contains("edit"));
    assert!(formatted.contains("src/main.rs"));
    assert!(formatted.contains("- old text"));
    assert!(formatted.contains("+ new text"));
    assert!(formatted.contains("Elapsed"));
}

#[test]
fn format_active_tool_block_includes_expand_hint_when_truncated() {
    let theme = crate::ui::theme::Theme::default();
    let output = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10";
    let formatted = super::format_active_tool_block(super::ActiveToolDisplayInput {
        tool_name: "bash",
        args_summary: "cargo test",
        preview: None,
        output,
        started: std::time::Instant::now(),
        theme: &theme,
        width: 60,
        expanded: false,
    });

    assert!(formatted.contains("earlier lines"));
    assert!(formatted.contains("Ctrl+O to expand"));
}
