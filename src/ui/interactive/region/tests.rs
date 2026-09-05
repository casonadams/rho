use super::{paint_lines, paint_region, region_style};
use crate::ui::theme::Theme;
use crate::ui::theme::definition::ThemeDef;

fn latte_theme() -> Theme {
    ThemeDef {
        background: Some("#eff1f5".into()),
        foreground: Some("#4c4f69".into()),
        green: Some("#40a02b".into()),
        ..Default::default()
    }
    .into_theme("latte-test")
}

#[test]
fn ansi_themes_leave_lines_untouched() {
    let theme = Theme::default();
    assert!(theme.is_ansi());
    assert_eq!(region_style(&theme), None);
    let lines = vec!["hello".to_string()];
    assert_eq!(paint_lines(&lines, &theme, 20), lines);
}

#[test]
fn hex_themes_paint_the_region_background_and_foreground() {
    let theme = latte_theme();
    assert!(!theme.is_ansi());
    let painted = paint_lines(&["editor text".to_string()], &theme, 20);
    assert_eq!(painted.len(), 1);
    let expected = "\x1b[38;2;76;79;105m\x1b[48;2;239;241;245meditor text\x1b[38;2;76;79;105m\x1b[48;2;239;241;245m         \x1b[0m";
    assert_eq!(painted[0], expected);
}

#[test]
fn empty_lines_become_full_width_theme_background() {
    let theme = latte_theme();
    let painted = paint_lines(&[String::new()], &theme, 10);
    assert_eq!(
        painted[0],
        "\x1b[38;2;76;79;105m\x1b[48;2;239;241;245m\x1b[38;2;76;79;105m\x1b[48;2;239;241;245m          \x1b[0m"
    );
}

#[test]
fn escape_only_lines_pass_through_untouched() {
    let theme = latte_theme();
    let marker = "\x1b]133;A\x1b\\".to_string();
    assert_eq!(
        paint_lines(std::slice::from_ref(&marker), &theme, 20),
        vec![marker.clone()]
    );
}

#[test]
fn inner_resets_reapply_the_region_colors() {
    let theme = latte_theme();
    let painted = paint_region("dim\x1b[0mbright", &theme, 40);
    let style_code = "\x1b[38;2;76;79;105m\x1b[48;2;239;241;245m";
    assert_eq!(
        painted,
        format!("{style_code}dim\x1b[0m{style_code}bright"),
        "open streamed lines stay unpadded with colors active"
    );
}

#[test]
fn complete_lines_are_padded_but_the_trailing_partial_line_is_not() {
    let theme = latte_theme();
    let painted = paint_region("full\ntok", &theme, 10);
    let style_code = "\x1b[38;2;76;79;105m\x1b[48;2;239;241;245m";
    assert_eq!(
        painted,
        format!("{style_code}full{style_code}{}\x1b[0m\n{style_code}tok", " ".repeat(6))
    );
}

#[test]
fn paint_region_preserves_line_count() {
    let theme = latte_theme();
    let lines = [
        String::new(),
        "one".to_string(),
        "\x1b]133;B\x1b\\".to_string(),
        "two".to_string(),
    ];
    let painted = paint_region(&lines.join("\n"), &theme, 15);
    assert_eq!(painted.split('\n').count(), lines.len());
}

#[test]
fn fg_only_theme_paints_the_foreground_without_padding() {
    let theme = ThemeDef {
        foreground: Some("#c0caf5".into()),
        ..Default::default()
    }
    .into_theme("fg-only");
    let painted = paint_lines(&["prose".to_string()], &theme, 20);
    assert_eq!(painted[0], "\x1b[38;2;192;202;245mprose\x1b[0m");
}

#[test]
fn wide_characters_count_toward_the_painted_width() {
    let theme = latte_theme();
    let painted = paint_lines(&["日本".to_string()], &theme, 10);
    let painted_line = &painted[0];
    let trailing_spaces = painted_line.matches(' ').count();
    assert_eq!(trailing_spaces, 6, "4 columns used by two wide glyphs");
}
