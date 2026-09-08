use super::terminal::{blend_fill, detect, theme_from_colorfbg};
use super::*;
use anstyle::{AnsiColor, Color, RgbColor};
use std::io::IsTerminal;

#[test]
fn default_block_fill_uses_terminal_black_background() {
    let theme = Theme::default();
    assert_eq!(theme.block_fill.render().to_string(), "\x1b[40m");
    assert!(matches!(
        theme.block_fill.get_bg_color(),
        Some(Color::Ansi(AnsiColor::Black))
    ));
}

fn assert_all_ansi_fg(styles: &[Style]) {
    for s in styles {
        assert!(matches!(s.get_fg_color(), Some(Color::Ansi(_))));
    }
}

#[test]
fn default_theme_uses_only_ansi_colors() {
    let theme = Theme::default();
    assert_all_ansi_fg(&[
        theme.prompt,
        theme.tool_header,
        theme.tool_ok,
        theme.tool_err,
        theme.highlight,
        theme.warning,
        theme.skill_tag,
    ]);
}

#[test]
fn secondary_text_uses_native_sgr2_dim_instead_of_palette_bright_black() {
    let theme = Theme::default();
    for style in [theme.dimmed, theme.thinking, theme.heading_h3] {
        assert_eq!(style.render().to_string(), "\x1b[2m");
        assert_eq!(style.get_fg_color(), None);
    }
}

#[test]
fn default_theme_has_no_rgb_colors_and_is_not_light() {
    let theme = Theme::default();
    assert!(!theme.is_light);
    for style in [
        theme.prompt,
        theme.thinking,
        theme.tool_header,
        theme.tool_ok,
        theme.tool_err,
        theme.highlight,
        theme.code_inline,
        theme.heading_h1,
        theme.heading_h2,
        theme.heading_h3,
        theme.dimmed,
        theme.warning,
        theme.skill_tag,
        theme.block_fill,
    ] {
        assert!(!matches!(style.get_fg_color(), Some(Color::Rgb(_))));
        assert!(!matches!(style.get_bg_color(), Some(Color::Rgb(_))));
    }
}

#[test]
fn blend_fill_tints_background_toward_foreground() {
    let fg = RgbColor(0xc0, 0xca, 0xf5);
    let bg = RgbColor(0x1e, 0x1e, 0x2e);
    let fill = match blend_fill(fg, bg).get_bg_color() {
        Some(Color::Rgb(rgb)) => rgb,
        other => panic!("expected rgb fill, got {other:?}"),
    };
    assert_eq!(fill, RgbColor(0x31, 0x32, 0x45));
}

#[test]
fn detect_without_a_tty_returns_the_default_theme() {
    // Falls back to the default theme when stdin/stdout are not interactive
    // TTYs; on a real TTY the query would run live and must not panic. When
    // COLORFGBG announces a mode, detect() styles through the base-16 slots
    // instead, so skip here and let the colorfbg tests cover it.
    if std::env::var_os("COLORFGBG").is_some() || (std::io::stdin().is_terminal() && std::io::stdout().is_terminal()) {
        return;
    }
    let detected = detect();
    assert_eq!(
        detected.block_fill.render().to_string(),
        Theme::default().block_fill.render().to_string()
    );
    assert!(!detected.is_light);
}

#[test]
fn tool_title_style_is_bold_and_red_on_error() {
    let theme = Theme::default();
    assert_eq!(theme.tool_title_style(false).render().to_string(), "\x1b[1m");
    assert_eq!(theme.tool_title_style(true).render().to_string(), "\x1b[1m\x1b[31m");
}

#[test]
fn colorfbg_dark_announcement_styles_through_base_16_slots() {
    let theme = theme_from_colorfbg("15;0").expect("dark theme");
    assert!(!theme.is_light);
    assert_eq!(theme.dimmed.render().to_string(), "\x1b[90m");
    assert_eq!(theme.block_fill.render().to_string(), "\x1b[40m");
    assert_eq!(theme.thinking, theme.dimmed);
    assert_eq!(theme.heading_h3, theme.dimmed);
    assert_eq!(theme.prompt, Theme::default().prompt);
}

#[test]
fn colorfbg_light_announcement_styles_through_base_16_slots() {
    let theme = theme_from_colorfbg("0;15").expect("light theme");
    assert!(theme.is_light);
    assert_eq!(theme.dimmed.render().to_string(), "\x1b[90m");
    assert_eq!(theme.block_fill.render().to_string(), "\x1b[40m");
}

#[test]
fn colorfbg_last_field_is_the_background_index() {
    assert!(!theme_from_colorfbg("7;0").expect("dark").is_light);
    assert!(theme_from_colorfbg("0;15").expect("light").is_light);
}

#[test]
fn colorfbg_garbage_falls_through() {
    assert_eq!(theme_from_colorfbg("nope"), None);
    assert_eq!(theme_from_colorfbg(""), None);
}
