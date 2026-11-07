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
