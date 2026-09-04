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
        autocomplete: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &widgets,
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert_eq!(layout.widget_lines.len(), 3);
    // 1 (editor) + 0 (queued) + 2 (footer) + 2 (status/spacer) + 1 (top_divider) + 1 (bottom_divider) + 3 (widgets) + 1 (spacer) = 11
    assert_eq!(layout.height(), 11);
    // cursor_row: 0 (queued) + 4 (widgets + spacer) + 3 (status/spacer + top_divider) + 0 (cursor.row) = 7
    assert_eq!(layout.cursor_row(), 7);
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
fn empty_editor_has_one_line_and_fixed_chrome() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        autocomplete: None,
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
    assert_eq!(layout.height(), 7);
}

#[test]
fn explicit_newlines_grow_the_editor() {
    let mut editor = EditorState::default();
    editor.set_text("one\ntwo\n");
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 20,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines, ["one", "two", ""]);
    assert_eq!(layout.cursor, CursorPosition { row: 2, column: 0 });
    assert_eq!(layout.height(), 9);
}

#[test]
fn soft_wrap_uses_display_width_for_wide_unicode() {
    let mut editor = EditorState::default();
    editor.set_text("ab界c");
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
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
        autocomplete: None,
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
        autocomplete: None,
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
        autocomplete: None,
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
        autocomplete: None,
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
        autocomplete: None,
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
        autocomplete: None,
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
        autocomplete: None,
        footer: &footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert_eq!(layout.working_line, "");
    assert_eq!(layout.height(), 7);
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
        autocomplete: None,
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
        autocomplete: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines.len(), 1);
    assert_eq!(layout.height(), 7);
    assert_eq!(layout.cursor_row(), 3);
}

#[test]
fn multiline_editor_height_matches_content() {
    let mut editor = EditorState::default();
    editor.set_text("line1\nline2\nline3");
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines.len(), 3);
    assert_eq!(layout.height(), 9);
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
        autocomplete: None,
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
    let modal = crate::ui::interactive::ModalState::new(
        "Select Model",
        "",
        vec![
            crate::ui::interactive::ModalOption::new("model-a", Some("openai\t✓\tdefault\t128k ctx")),
            crate::ui::interactive::ModalOption::new("model-b", Some("anthropic\t\t\t200k ctx")),
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
        (Some("xhigh"), "\u{1b}[31m"),
        (Some("max"), "\u{1b}[1;31m"),
    ];

    for (level, expected_style) in levels {
        let footer = FooterState {
            thinking_level: level.map(ToString::to_string),
            ..FooterState::default()
        };
        let layout = layout(LayoutInput {
            editor: &default_editor,
            modal: None,
            autocomplete: None,
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

#[test]
fn bash_mode_border_turns_amber() {
    let mut editor = EditorState::default();
    editor.set_text("!cargo check");
    let footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 10,
        spinner_frame: 0,
    });

    assert!(layout.top_divider.starts_with("\u{1b}[33m"));
    assert!(layout.bottom_divider.starts_with("\u{1b}[33m"));
}

#[test]
fn top_divider_shows_name_and_version_when_label_enabled() {
    let editor = EditorState::default();
    let footer = FooterState {
        show_label: true,
        ..FooterState::default()
    };
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 25,
        spinner_frame: 0,
    });

    let stripped = crate::ui::interactive::footer::visible_width(&layout.top_divider);
    assert_eq!(stripped, 25, "divider must stay exactly one terminal row wide");
    let label = concat!("rho ", env!("CARGO_PKG_VERSION"));
    assert!(layout.top_divider.contains(label));
    assert!(layout.bottom_divider.contains("─") && !layout.bottom_divider.contains("rho"));
}

#[test]
fn top_divider_shows_nothing_by_default() {
    let editor = EditorState::default();
    let footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 25,
        spinner_frame: 0,
    });

    let stripped = crate::ui::interactive::footer::visible_width(&layout.top_divider);
    assert_eq!(stripped, 25);
    assert_eq!(layout.top_divider.matches('─').count(), 25);
    assert!(!layout.top_divider.contains("rho"));
}

#[test]
fn top_divider_falls_back_to_plain_dashes_when_narrow() {
    let editor = EditorState::default();
    let footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 6,
        spinner_frame: 0,
    });

    assert!(!layout.top_divider.contains("rho"));
}

#[test]
fn running_tool_widget_renders_header_tail_and_elapsed() {
    let theme = crate::ui::theme::Theme::default();
    let mut tool = crate::ui::interactive::state::RunningTool::new("bash", "cargo test", None);
    tool.append_chunk("line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\n");

    // Collapsed view (tools_expanded = false)
    let lines = super::render_running_tool_widget(super::RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    let full = lines.join("\n");
    assert!(full.contains("bash"), "should contain tool name");
    assert!(full.contains("cargo test"), "should contain command");
    assert!(
        full.contains("... (2 earlier lines, Ctrl+O to expand)"),
        "should show skipped lines count"
    );
    assert!(full.contains("line 7"), "should show latest tailed lines");
    assert!(
        !full.contains("line 1\n"),
        "earlier line 1 should be truncated from tail preview"
    );
    assert!(full.contains("Elapsed"), "should contain elapsed duration");

    // Expanded view (tools_expanded = true)
    let lines_expanded = super::render_running_tool_widget(super::RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: true,
    });
    let full_expanded = lines_expanded.join("\n");
    assert!(
        full_expanded.contains("line 1"),
        "expanded view should show earlier lines"
    );
    assert!(full_expanded.contains("line 7"));
    assert!(
        !full_expanded.contains("earlier lines, Ctrl+O"),
        "expanded view should not have skip hint"
    );
}

#[test]
fn running_tool_widget_with_preview_renders_diff_card() {
    let theme = crate::ui::theme::Theme::default();
    let preview = Some("+ line added\n- line removed".to_string());
    let tool = crate::ui::interactive::state::RunningTool::new("edit", "src/main.rs", preview);

    let lines = super::render_running_tool_widget(super::RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    let full = lines.join("\n");
    assert!(full.contains("edit"));
    assert!(full.contains("src/main.rs"));
    assert!(full.contains("+ line added"));
    assert!(full.contains("- line removed"));
    assert!(full.contains("Elapsed"));
}

#[test]
fn running_tool_widget_empty_for_fast_tools_without_preview_or_output() {
    let theme = crate::ui::theme::Theme::default();
    let tool_fd = crate::ui::interactive::state::RunningTool::new("fd", "pattern in .", None);
    let lines_fd = super::render_running_tool_widget(super::RunningToolWidgetInput {
        tool: &tool_fd,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    assert!(lines_fd.is_empty(), "fd should not render a running widget card");

    let tool_rg = crate::ui::interactive::state::RunningTool::new("rg", "/pattern/ in .", None);
    let lines_rg = super::render_running_tool_widget(super::RunningToolWidgetInput {
        tool: &tool_rg,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    assert!(lines_rg.is_empty(), "rg should not render a running widget card");
}

#[test]
fn running_tool_widget_large_output_pre_slicing_and_skipped_count() {
    let theme = crate::ui::theme::Theme::default();
    let mut tool = crate::ui::interactive::state::RunningTool::new("bash", "seq 1 120", None);
    let mut output = String::new();
    for i in 1..=120 {
        output.push_str(&format!("line {i}\n"));
    }
    tool.append_chunk(&output);

    // Collapsed view: pre-slicing activates (120 > 50).
    // Total lines = 120, shown visual lines = 5 (lines 116..120), skipped = 115.
    let lines = super::render_running_tool_widget(super::RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    let full = lines.join("\n");
    assert!(
        full.contains("... (115 earlier lines, Ctrl+O to expand)"),
        "expected 115 skipped lines, got:\n{full}"
    );
    assert!(full.contains("line 116"));
    assert!(full.contains("line 120"));
    assert!(
        !full.contains("line 1\n") && !full.contains("line 50\n") && !full.contains("line 100\n"),
        "earlier lines should not be rendered in collapsed view"
    );

    // Expanded view: renders all lines
    let lines_expanded = super::render_running_tool_widget(super::RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: true,
    });
    let full_expanded = lines_expanded.join("\n");
    assert!(full_expanded.contains("line 1"));
    assert!(full_expanded.contains("line 120"));
    assert!(!full_expanded.contains("earlier lines, Ctrl+O to expand"));
}

#[test]
fn running_tool_widget_pre_slice_boundary_50_and_51_lines() {
    let theme = crate::ui::theme::Theme::default();

    // Exactly 50 lines: does not pre-slice (boundary is > 50).
    let mut tool_50 = crate::ui::interactive::state::RunningTool::new("bash", "seq 1 50", None);
    let mut out_50 = String::new();
    for i in 1..=50 {
        out_50.push_str(&format!("line {i}\n"));
    }
    tool_50.append_chunk(&out_50);

    let lines_50 = super::render_running_tool_widget(super::RunningToolWidgetInput {
        tool: &tool_50,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    let full_50 = lines_50.join("\n");
    assert!(
        full_50.contains("... (45 earlier lines, Ctrl+O to expand)"),
        "50 lines should show 45 earlier lines (50 - 5)"
    );
    assert!(full_50.contains("line 50"));

    // 51 lines: triggers pre-slicing (51 > 50).
    let mut tool_51 = crate::ui::interactive::state::RunningTool::new("bash", "seq 1 51", None);
    let mut out_51 = String::new();
    for i in 1..=51 {
        out_51.push_str(&format!("line {i}\n"));
    }
    tool_51.append_chunk(&out_51);

    let lines_51 = super::render_running_tool_widget(super::RunningToolWidgetInput {
        tool: &tool_51,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    let full_51 = lines_51.join("\n");
    assert!(
        full_51.contains("... (46 earlier lines, Ctrl+O to expand)"),
        "51 lines should show 46 earlier lines (51 - 5)"
    );
    assert!(full_51.contains("line 47"));
    assert!(full_51.contains("line 51"));
    assert!(!full_51.contains("line 46\n"));
}

#[test]
fn running_tool_widget_large_output_with_soft_wrapping() {
    let theme = crate::ui::theme::Theme::default();
    let mut tool = crate::ui::interactive::state::RunningTool::new("bash", "wrapped", None);
    let mut output = String::new();
    for i in 1..=55 {
        output.push_str(&format!("line {i}\n"));
    }
    // Add a wide line at line 56 that wraps into 2 visual lines at width 30
    output
        .push_str("line 56: this is a very long line that will definitely wrap across multiple visual terminal rows\n");
    tool.append_chunk(&output);

    let lines = super::render_running_tool_widget(super::RunningToolWidgetInput {
        tool: &tool,
        theme: &theme,
        width: 60,
        tools_expanded: false,
    });
    let full = lines.join("\n");
    // 56 total logical lines. Line 56 wraps into 4 visual lines at inner width 26.
    // 55 single lines + 4 wrapped lines = 59 visual lines. Showing 5 visual lines means 54 skipped.
    assert!(full.contains("earlier lines, Ctrl+O to expand"));
    assert!(full.contains("line 56"));
}
