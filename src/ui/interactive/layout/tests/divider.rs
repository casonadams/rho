use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState};

fn layout_for_thinking(level: Option<&str>) -> crate::ui::interactive::layout::InteractiveLayout {
    let footer = FooterState {
        thinking_level: level.map(ToString::to_string),
        ..FooterState::default()
    };
    layout(LayoutInput {
        editor: &EditorState::default(),
        modal: None,
        autocomplete: None,
        footer: &footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 10,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
        focused: true,
    })
}

#[test]
fn thinking_borders_change_color_with_thinking_level() {
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
        let layout = layout_for_thinking(level);
        assert!(layout.top_divider.starts_with(expected_style));
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
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 10,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
        focused: true,
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
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 25,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
        focused: true,
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
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 25,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
        focused: true,
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
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 6,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
        focused: true,
    });

    assert!(!layout.top_divider.contains("rho"));
}

fn modal_layout_with_mode(
    modal: &crate::ui::interactive::ModalState,
) -> crate::ui::interactive::layout::InteractiveLayout {
    layout(LayoutInput {
        editor: &EditorState::default(),
        modal: Some(modal),
        autocomplete: None,
        footer: &FooterState::default(),
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
        focused: true,
    })
}

#[test]
fn modal_top_divider_reflects_active_input_mode() {
    let mut modal = crate::ui::interactive::ModalState::new("Permission Required", "body", vec![]);
    assert!(
        modal_layout_with_mode(&modal)
            .top_divider
            .contains("Permission Required")
    );
    let modes = [
        ("edit", "edit"),
        ("args", "edit"),
        ("reason", "reason"),
        ("pattern", "pattern"),
    ];
    for (mode_label, expected_title) in modes {
        modal.mode = crate::ui::interactive::ModalMode::Input {
            prompt_label: mode_label.to_string(),
        };
        let l = modal_layout_with_mode(&modal);
        assert!(l.top_divider.contains(expected_title) && !l.top_divider.contains("Permission Required"));
    }
}
