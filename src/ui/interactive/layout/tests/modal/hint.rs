use crate::ui::interactive::layout::{LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState, ModalOption, ModalState};
use crate::ui::theme::Theme;

fn modal_hint_layout(theme: &Theme) -> crate::ui::interactive::layout::InteractiveLayout {
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
    let theme = Theme::default();
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
    let theme = Theme {
        dimmed: anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Magenta))),
        ..Default::default()
    };
    let magenta_dim = theme.dimmed.render().to_string();
    let layout = modal_hint_layout(&theme);

    let bottom_line = layout.lines.last().expect("bottom line exists");
    assert!(bottom_line.contains(&magenta_dim));
}
