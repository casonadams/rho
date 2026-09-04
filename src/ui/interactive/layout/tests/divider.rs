use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState};

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
    assert!(layout.bottom_divider.contains('─') && !layout.bottom_divider.contains("rho"));
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
