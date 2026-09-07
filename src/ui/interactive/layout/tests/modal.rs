//! Modal layout tests: bounds/truncation, cursor placement, expansion,
//! hints, horizontal option rendering, and body scrolling.

use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState, ModalOption, ModalState};

// ---------------------------------------------------------------------------
// Bounds and truncation
// ---------------------------------------------------------------------------

fn sample_permission_modal(n: usize) -> ModalState {
    let body = (1..=n)
        .map(|i| format!("command argument line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    ModalState::new(
        "Permission Required",
        &body,
        vec![ModalOption::from("Allow"), ModalOption::from("Deny")],
    )
}

fn modal_test_layout(modal: &ModalState, terminal_height: usize) -> crate::ui::interactive::layout::InteractiveLayout {
    layout(LayoutInput {
        editor: &EditorState::default(),
        modal: Some(modal),
        autocomplete: None,
        footer: &FooterState::default(),
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height,
        spinner_frame: 0,
        theme: None,
    })
}

fn parse_omitted_count(line: &str) -> usize {
    line.split("[... ")
        .nth(1)
        .and_then(|s| s.split(" lines omitted").next())
        .expect("extract count")
        .parse()
        .expect("parse count")
}

#[test]
fn modal_body_truncation_on_small_terminal() {
    let modal = sample_permission_modal(30);
    let layout = modal_test_layout(&modal, 15);

    assert!(layout.lines.len() <= 15);
    let allow_idx = layout.lines.iter().position(|l| l.contains("Allow")).unwrap();
    let deny_idx = layout.lines.iter().position(|l| l.contains("Deny")).unwrap();
    let omitted_idx = layout.lines.iter().position(|l| l.contains("lines omitted")).unwrap();
    assert!(omitted_idx < allow_idx && allow_idx < deny_idx);

    let rendered_lines = layout
        .lines
        .iter()
        .filter(|l| l.contains("command argument line"))
        .count();
    let omitted_count = parse_omitted_count(&layout.lines[omitted_idx]);
    assert_eq!(rendered_lines + omitted_count, 30);
    assert!(layout.bottom_divider.contains("─".repeat(80).as_str()));
}

#[test]
fn modal_body_truncation_minimal_omitted_lines() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let body = (1..=9).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let modal = ModalState::new(
        "Permission",
        &body,
        vec![ModalOption::from("Allow"), ModalOption::from("Deny")],
    );

    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: Some(&modal),
        autocomplete: None,
        footer: &default_footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 15,
        spinner_frame: 0,
        theme: None,
    });

    assert!(layout.lines.len() <= 15);
    assert!(layout.lines.iter().any(|l| l.contains("2 lines omitted")));
}

#[test]
fn modal_body_fits_minimal_on_terminal_height_8() {
    let modal = sample_permission_modal(30);
    let layout_8 = modal_test_layout(&modal, 8);
    assert!(layout_8.lines.len() <= 8);
    assert!(layout_8.lines.iter().any(|l| l.contains("Allow")));
    assert!(layout_8.lines.iter().any(|l| l.contains("Deny")));
}

#[test]
fn modal_body_suppressed_on_minimal_terminal_height_6() {
    let modal = sample_permission_modal(30);
    let layout_6 = modal_test_layout(&modal, 6);
    assert!(layout_6.lines.len() <= 6);
    assert!(layout_6.lines.iter().any(|l| l.contains("Allow")));
    assert!(layout_6.bottom_divider.contains("─".repeat(80).as_str()));
    assert!(!layout_6.lines.iter().any(|l| l.contains("command argument line")));
}

// ---------------------------------------------------------------------------
// Cursor placement
// ---------------------------------------------------------------------------

fn modal_cursor_layout(modal: &ModalState) -> crate::ui::interactive::layout::InteractiveLayout {
    layout(LayoutInput {
        editor: &EditorState::default(),
        modal: Some(modal),
        autocomplete: None,
        footer: &FooterState::default(),
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 15,
        spinner_frame: 0,
        theme: None,
    })
}

fn assert_modal_cursor_valid(layout: &crate::ui::interactive::layout::InteractiveLayout, expected_content: &str) {
    assert!(layout.lines.len() <= 15 && layout.cursor_visible);
    assert!(layout.cursor_row < layout.lines.len() && layout.cursor.column <= 80);
    assert!(layout.lines[layout.cursor_row].contains(expected_content));
}

#[test]
fn modal_input_mode_cursor_with_body_truncation() {
    let body = (1..=30)
        .map(|i| format!("long description line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut modal = ModalState::new("Reason Required", &body, vec![ModalOption::from("Confirm")]);
    modal.mode = crate::ui::interactive::ModalMode::Input {
        prompt_label: "Reason".to_string(),
    };
    modal.input.set_text("test reason");

    let layout = modal_cursor_layout(&modal);
    assert_modal_cursor_valid(&layout, "test reason");
    assert!(layout.top_divider.contains("Reason"));
}

#[test]
fn modal_searchable_cursor_with_body_truncation() {
    let body = (1..=30)
        .map(|i| format!("model detail line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let modal = ModalState::new("Select Model", &body, vec![ModalOption::from("model-1")]).with_search(true);

    let layout = modal_cursor_layout(&modal);
    assert_eq!(layout.cursor_row, 1);
    assert_modal_cursor_valid(&layout, ">");
}

// ---------------------------------------------------------------------------
// Expansion
// ---------------------------------------------------------------------------

#[test]
fn test_in_input_modal_desired_lines_uncapped_body() {
    let body = (1..=20).map(|i| format!("cmd {i}")).collect::<Vec<_>>().join("\n");
    let mut modal = ModalState::new("Run", &body, vec![ModalOption::from("Allow")]);
    modal.option_layout = crate::ui::interactive::OptionLayout::Horizontal;
    let desired = crate::ui::interactive::layout::modal::in_input::in_input_modal_desired_lines(&modal, "", 80);
    assert_eq!(desired, 21);
}

#[test]
fn test_calculate_content_space_horizontal_layout() {
    use crate::ui::interactive::layout::modal::in_input::calculate_content_space;
    let mut modal = ModalState::new("Run", "body", vec![ModalOption::from("Allow")]);
    modal.option_layout = crate::ui::interactive::OptionLayout::Horizontal;
    assert_eq!(calculate_content_space(&modal, 0), (0, 0));
    assert_eq!(calculate_content_space(&modal, 1), (0, 1));
    assert_eq!(calculate_content_space(&modal, 5), (4, 1));
    assert_eq!(calculate_content_space(&modal, 15), (14, 1));
}

#[test]
fn test_calculate_content_space_vertical_layout() {
    use crate::ui::interactive::layout::modal::in_input::calculate_content_space;
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
    modal.option_layout = crate::ui::interactive::OptionLayout::Horizontal;
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
        modal.option_layout = crate::ui::interactive::OptionLayout::Horizontal;
        let rendered = build_layout_with_modal(&modal, height);
        let bot_div_idx = rendered.lines.iter().rposition(|l| l.contains("────────")).unwrap();
        assert_eq!(bot_div_idx, rendered.lines.len() - 2);
    }
}

// ---------------------------------------------------------------------------
// Hints
// ---------------------------------------------------------------------------

fn modal_hint_layout(theme: &crate::ui::theme::Theme) -> crate::ui::interactive::layout::InteractiveLayout {
    let modal = ModalState::new("Select Model", "", vec![ModalOption::from("model-a")]).with_search(true);
    layout(LayoutInput {
        editor: &EditorState::default(),
        modal: Some(&modal),
        autocomplete: None,
        footer: &FooterState::default(),
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: Some(theme),
    })
}

#[test]
fn modal_hint_matches_footer_dimmed_style_without_raw_faint_escape() {
    let theme = crate::ui::theme::Theme::default();
    let dimmed = theme.dimmed.render().to_string();
    let layout = modal_hint_layout(&theme);

    assert_eq!(layout.footer_lines.len(), 1);
    assert_eq!(
        layout.footer_lines[0],
        "Enter to select • Ctrl+S to set as default • Esc to cancel"
    );

    let bottom_line = layout.lines.last().expect("bottom line exists");
    assert!(bottom_line.contains(&dimmed) && !bottom_line.contains("\x1b[2m"));
}

#[test]
fn modal_hint_adopts_custom_theme_dimmed_color() {
    let theme = crate::ui::theme::Theme {
        dimmed: anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Magenta))),
        ..Default::default()
    };
    let magenta_dim = theme.dimmed.render().to_string();
    let layout = modal_hint_layout(&theme);

    let bottom_line = layout.lines.last().expect("bottom line exists");
    assert!(bottom_line.contains(&magenta_dim));
}

// ---------------------------------------------------------------------------
// Horizontal option rendering
// ---------------------------------------------------------------------------

mod horizontal {
    use crate::ui::interactive::layout::modal::modal_hint;
    use crate::ui::interactive::layout::modal::options::{ModalOptionsLayout, render_modal_options};
    use crate::ui::interactive::{ModalMode, ModalOption, ModalState, OptionLayout};
    use crate::ui::theme::Theme;

    fn sample_permission_modal() -> ModalState {
        let mut modal = ModalState::new(
            "Permission Required",
            "Run echo hello",
            vec![
                ModalOption::from("Allow"),
                ModalOption::from("Edit"),
                ModalOption::from("Always"),
                ModalOption::from("Deny"),
            ],
        );
        modal.option_layout = OptionLayout::Horizontal;
        modal
    }

    #[test]
    fn test_horizontal_options_all_fit_rendering() {
        let modal = sample_permission_modal();
        let theme = Theme::default();
        let lines = render_modal_options(
            &modal,
            ModalOptionsLayout {
                inner_width: 80,
                max_visible: 1,
                theme: &theme,
            },
        );
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert!(line.contains("Allow") && line.contains("Edit") && line.contains("Always") && line.contains("Deny"));
        assert!(line.contains('▸'));
        assert!(!line.contains('‹') && !line.contains('›'));
    }

    #[test]
    fn test_horizontal_options_selection_marker_moves() {
        let mut modal = sample_permission_modal();
        modal.selected = 1;
        let theme = Theme::default();
        let lines = render_modal_options(
            &modal,
            ModalOptionsLayout {
                inner_width: 80,
                max_visible: 1,
                theme: &theme,
            },
        );
        let line = &lines[0];
        assert!(line.contains("▸") && line.contains("Edit"));
        assert!(!line.contains("▸ Allow"));
    }

    #[test]
    fn test_horizontal_options_overflow_right_indicator() {
        let modal = sample_permission_modal();
        let theme = Theme::default();
        let lines = render_modal_options(
            &modal,
            ModalOptionsLayout {
                inner_width: 16,
                max_visible: 1,
                theme: &theme,
            },
        );
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert!(line.contains("Allow"));
        assert!(line.contains('›'));
        assert!(!line.contains('‹'));
    }

    #[test]
    fn test_horizontal_options_overflow_left_indicator() {
        let mut modal = sample_permission_modal();
        modal.selected = 3;
        let theme = Theme::default();
        let lines = render_modal_options(
            &modal,
            ModalOptionsLayout {
                inner_width: 16,
                max_visible: 1,
                theme: &theme,
            },
        );
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert!(line.contains("Deny"));
        assert!(line.contains('‹'));
        assert!(!line.contains('›'));
    }

    #[test]
    fn test_horizontal_options_overflow_both_indicators() {
        let mut modal = sample_permission_modal();
        modal.selected = 2;
        let theme = Theme::default();
        let lines = render_modal_options(
            &modal,
            ModalOptionsLayout {
                inner_width: 14,
                max_visible: 1,
                theme: &theme,
            },
        );
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert!(line.contains("Always"));
        assert!(line.contains('‹') || line.contains('›'));
    }

    #[test]
    fn test_horizontal_modal_hints_for_select_and_input_modes() {
        let mut modal = sample_permission_modal();
        assert_eq!(
            modal_hint(&modal),
            "←/→ or h/l select • ↑/↓ or j/k scroll • Enter confirm • Esc deny"
        );
        modal.mode = ModalMode::Input {
            prompt_label: "cmd".into(),
        };
        assert_eq!(modal_hint(&modal), "Enter submit • Esc back");
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Body scrolling
// ---------------------------------------------------------------------------

fn sample_scrollable_modal(body_lines: usize, option_layout: crate::ui::interactive::OptionLayout) -> ModalState {
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
    let modal = sample_scrollable_modal(30, crate::ui::interactive::OptionLayout::Horizontal);
    let rendered = render_modal_layout(&modal, 15);
    assert!(rendered.lines.iter().any(|l| l.contains("↑/↓ scroll (line 1/30)")));
    assert!(rendered.lines.iter().any(|l| l.trim() == "command_arg_line_1"));
}

#[test]
fn test_horizontal_modal_body_scroll_offset() {
    let mut modal = sample_scrollable_modal(30, crate::ui::interactive::OptionLayout::Horizontal);
    modal.body_scroll = 5;
    let rendered = render_modal_layout(&modal, 15);
    assert!(rendered.lines.iter().any(|l| l.contains("↑/↓ scroll (line 6/30)")));
    assert!(!rendered.lines.iter().any(|l| l.trim() == "command_arg_line_1"));
    assert!(rendered.lines.iter().any(|l| l.trim() == "command_arg_line_6"));
}

#[test]
fn test_horizontal_modal_body_scroll_clamped_at_end() {
    let mut modal = sample_scrollable_modal(30, crate::ui::interactive::OptionLayout::Horizontal);
    modal.body_scroll = 500;
    let rendered = render_modal_layout(&modal, 15);
    assert!(rendered.lines.iter().any(|l| l.contains("↑/↓ scroll (line ")));
    assert!(rendered.lines.iter().any(|l| l.trim() == "command_arg_line_30"));
}

#[test]
fn test_horizontal_modal_body_fits_no_scroll_indicator() {
    let modal = sample_scrollable_modal(3, crate::ui::interactive::OptionLayout::Horizontal);
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
    let modal = sample_scrollable_modal(30, crate::ui::interactive::OptionLayout::Vertical);
    let rendered = render_modal_layout(&modal, 15);
    assert!(rendered.lines.iter().any(|l| l.contains("lines omitted")));
    assert!(!rendered.lines.iter().any(|l| l.contains("↑/↓ scroll")));
}
