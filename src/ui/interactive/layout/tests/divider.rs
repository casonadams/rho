use super::base_input;
use crate::ui::block::visible_width;
use crate::ui::interactive::layout::{InteractiveLayout, LayoutInput, layout};
use crate::ui::interactive::{Activity, EditorState, FooterState, ModalMode, ModalState};

fn render(editor: &EditorState, footer: &FooterState, modal: Option<&ModalState>, width: usize) -> InteractiveLayout {
    layout(LayoutInput {
        modal,
        terminal_width: width,
        ..base_input(editor, footer)
    })
}

fn footer_with_thinking(level: Option<&str>) -> FooterState {
    FooterState {
        thinking_level: level.map(ToString::to_string),
        ..FooterState::default()
    }
}

#[test]
fn border_colors_follow_thinking_level_and_bash_mode() {
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
        let layout = render(&EditorState::default(), &footer_with_thinking(level), None, 10);
        assert!(layout.top_divider.starts_with(expected_style));
        assert!(layout.bottom_divider.starts_with(expected_style));
    }

    let mut editor = EditorState::default();
    editor.set_text("!cargo check");
    let layout = render(&editor, &FooterState::default(), None, 10);

    assert!(layout.top_divider.starts_with("\u{1b}[33m"));
    assert!(layout.bottom_divider.starts_with("\u{1b}[33m"));
}

#[test]
fn auto_thinking_divider_uses_resolved_level_color_during_turn() {
    use rho_harness_core::thinking::ThinkingLevel;

    let cases = [
        (None, "\u{1b}[33m"),
        (Some(ThinkingLevel::Low), "\u{1b}[34m"),
        (Some(ThinkingLevel::Medium), "\u{1b}[36m"),
        (Some(ThinkingLevel::High), "\u{1b}[35m"),
    ];

    for (resolved, expected_style) in cases {
        let mut footer = footer_with_thinking(Some("auto"));
        footer.resolved_thinking_level = resolved;
        let layout = render(&EditorState::default(), &footer, None, 10);
        assert!(
            layout.top_divider.starts_with(expected_style),
            "resolved={resolved:?}, got={:?}",
            layout.top_divider
        );
        assert!(
            layout.bottom_divider.starts_with(expected_style),
            "resolved={resolved:?}, got={:?}",
            layout.bottom_divider
        );
    }
}

#[test]
fn top_divider_shows_name_and_version_when_label_enabled() {
    let footer = FooterState {
        show_label: true,
        ..FooterState::default()
    };
    let layout = render(&EditorState::default(), &footer, None, 25);

    let stripped = visible_width(&layout.top_divider);
    assert_eq!(stripped, 25, "divider must stay exactly one terminal row wide");
    let label = concat!("rho ", env!("CARGO_PKG_VERSION"));
    assert!(layout.top_divider.contains(label));
    assert!(layout.bottom_divider.contains('─') && !layout.bottom_divider.contains("rho"));

    let wide = render(&EditorState::default(), &footer, None, 80);
    assert_eq!(visible_width(&wide.top_divider), 80);
    assert!(wide.top_divider.contains(rho_harness_core::build_info::divider_label()));
}

#[test]
fn top_divider_without_label_is_plain_dashes() {
    for width in [25, 6] {
        let layout = render(&EditorState::default(), &FooterState::default(), None, width);
        assert_eq!(visible_width(&layout.top_divider), width);
        assert!(!layout.top_divider.contains("rho"));
    }
    let layout = render(&EditorState::default(), &FooterState::default(), None, 25);
    assert_eq!(layout.top_divider.matches('─').count(), 25);
}

#[test]
fn modal_top_divider_reflects_active_input_mode() {
    let mut modal = ModalState::new("Permission Required", "body", vec![]);
    let layout = render(&EditorState::default(), &FooterState::default(), Some(&modal), 80);
    assert!(layout.top_divider.contains("Permission Required"));
    let modes = [
        ("edit", "edit"),
        ("args", "edit"),
        ("reason", "reason"),
        ("pattern", "pattern"),
    ];
    for (mode_label, expected_title) in modes {
        modal.mode = ModalMode::Input {
            prompt_label: mode_label.to_string(),
        };
        let l = render(&EditorState::default(), &FooterState::default(), Some(&modal), 80);
        assert!(l.top_divider.contains(expected_title) && !l.top_divider.contains("Permission Required"));
    }
}

#[test]
fn top_divider_busy_renders_spinner_and_activity_matching_thinking_color() {
    let footer = FooterState {
        activity: Activity::Working,
        thinking_level: Some("medium".into()),
        show_label: true,
        ..FooterState::default()
    };
    let layout = render(&EditorState::default(), &footer, None, 60);

    assert!(layout.top_divider.starts_with("\x1b[36m"));
    assert!(layout.top_divider.contains("working"));
    assert!(layout.top_divider.contains('\u{280b}'));
    assert!(layout.top_divider.contains("── \u{280b} working "));
    let label = concat!("rho ", env!("CARGO_PKG_VERSION"));
    assert!(layout.top_divider.contains(label));
    assert!(layout.top_divider.contains("───"));
    let stripped = visible_width(&layout.top_divider);
    assert_eq!(stripped, 60, "busy divider must match terminal width");
}

#[test]
fn top_divider_busy_degrades_with_width() {
    let footer = FooterState {
        activity: Activity::Working,
        show_label: true,
        ..FooterState::default()
    };
    let cases: [(usize, &[&str], &[&str]); 4] = [
        (20, &["working", "\u{280b}", "── \u{280b} working "], &["rho"]),
        (10, &["\u{280b}"], &["working"]),
        (7, &["── \u{280b} ──"], &["working"]),
        (5, &[], &["\u{280b}"]),
    ];
    for (width, present, absent) in cases {
        let layout = render(&EditorState::default(), &footer, None, width);
        for needle in present {
            assert!(layout.top_divider.contains(needle), "width {width} missing {needle:?}");
        }
        for needle in absent {
            assert!(
                !layout.top_divider.contains(needle),
                "width {width} unexpectedly has {needle:?}"
            );
        }
        assert_eq!(visible_width(&layout.top_divider), width);
    }
}

#[test]
fn prompt_show_version_false_omits_version_label() {
    use rho_harness_core::config::PromptUiConfig;

    let footer = FooterState {
        show_label: true,
        prompt_config: Some(PromptUiConfig {
            show_version: Some(false),
            ..PromptUiConfig::default()
        }),
        ..FooterState::default()
    };
    let layout = render(&EditorState::default(), &footer, None, 60);
    assert_eq!(visible_width(&layout.top_divider), 60);
    assert!(!layout.top_divider.contains("rho"));
}

#[test]
fn prompt_divider_label_renders_custom_unicode_glyph_with_exact_width() {
    use rho_harness_core::config::PromptUiConfig;

    let idle_footer = FooterState {
        show_label: true,
        prompt_config: Some(PromptUiConfig {
            show_version: Some(false),
            divider_label: Some("ϱ".to_string()),
            ..PromptUiConfig::default()
        }),
        ..FooterState::default()
    };
    let idle = render(&EditorState::default(), &idle_footer, None, 40);
    assert_eq!(visible_width(&idle.top_divider), 40);
    assert!(idle.top_divider.contains(" ϱ ───"));
    assert!(!idle.top_divider.contains("rho"));

    let busy_footer = FooterState {
        activity: Activity::Working,
        ..idle_footer
    };
    let busy = render(&EditorState::default(), &busy_footer, None, 40);
    assert_eq!(visible_width(&busy.top_divider), 40);
    assert!(busy.top_divider.contains("working"));
    assert!(busy.top_divider.contains(" ϱ ───"));
}

#[test]
fn divider_style_dim_and_none_behave_cleanly() {
    use rho_harness_core::config::{DividerStyle, PromptUiConfig};

    let dim_footer = FooterState {
        thinking_level: Some("high".into()),
        prompt_config: Some(PromptUiConfig {
            divider_style: Some(DividerStyle::Dim),
            ..PromptUiConfig::default()
        }),
        ..FooterState::default()
    };
    let dim_layout = render(&EditorState::default(), &dim_footer, None, 20);
    assert!(dim_layout.top_divider.starts_with("\x1b[2m"));

    let none_footer = FooterState {
        prompt_config: Some(PromptUiConfig {
            divider_style: Some(DividerStyle::None),
            ..PromptUiConfig::default()
        }),
        ..FooterState::default()
    };
    let none_layout = render(&EditorState::default(), &none_footer, None, 20);
    assert_eq!(none_layout.top_divider, "");
    assert_eq!(none_layout.bottom_divider, "");
    assert!(none_layout.cursor_row() < none_layout.height());
}

#[test]
fn context_line_embedded_renders_percentage_and_window_in_bottom_divider() {
    use crate::ui::block::strip_ansi;
    use rho_harness_core::config::{ContextLineMode, StatusBarConfig};

    let footer = FooterState {
        context_percent: Some(42.0),
        context_window: 200_000,
        status_bar_config: Some(StatusBarConfig {
            context_line: Some(ContextLineMode::Embedded),
            ..StatusBarConfig::default()
        }),
        ..FooterState::default()
    };
    let layout = render(&EditorState::default(), &footer, None, 40);
    assert_eq!(visible_width(&layout.bottom_divider), 40);
    let plain = strip_ansi(&layout.bottom_divider);
    assert!(plain.contains("42% / 200k"), "got {plain:?}");
}

#[test]
fn context_line_annotated_with_divider_style_none_ends_with_ansi_reset() {
    use rho_harness_core::config::{ContextLineMode, DividerStyle, PromptUiConfig, StatusBarConfig};

    let footer = FooterState {
        context_percent: Some(42.0),
        prompt_config: Some(PromptUiConfig {
            divider_style: Some(DividerStyle::None),
            ..PromptUiConfig::default()
        }),
        status_bar_config: Some(StatusBarConfig {
            context_line: Some(ContextLineMode::Annotated),
            ..StatusBarConfig::default()
        }),
        ..FooterState::default()
    };
    let layout = render(&EditorState::default(), &footer, None, 40);
    assert_eq!(visible_width(&layout.bottom_divider), 40);
    assert!(
        layout.bottom_divider.ends_with("\x1b[0m"),
        "got {:?}",
        layout.bottom_divider
    );
}

#[test]
fn context_line_percentage_with_divider_style_dim_distinguishes_used_and_rest() {
    use rho_harness_core::config::{ContextLineMode, DividerStyle, PromptUiConfig, StatusBarConfig};

    let footer = FooterState {
        context_percent: Some(25.0),
        prompt_config: Some(PromptUiConfig {
            divider_style: Some(DividerStyle::Dim),
            ..PromptUiConfig::default()
        }),
        status_bar_config: Some(StatusBarConfig {
            context_line: Some(ContextLineMode::Percentage),
            ..StatusBarConfig::default()
        }),
        ..FooterState::default()
    };
    let layout = render(&EditorState::default(), &footer, None, 40);
    assert_eq!(
        layout.bottom_divider,
        format!("\x1b[32m{}\x1b[2m{}\x1b[0m", "─".repeat(10), "─".repeat(30))
    );
}
