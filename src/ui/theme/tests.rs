use super::color::parse_color;
use super::definition::ThemeDef;
use super::registry::ThemeRegistry;
use super::*;
use anstyle::{AnsiColor, Color, RgbColor};

#[test]
fn block_backgrounds_use_only_terminal_ansi_colors() {
    let theme = Theme::default();
    assert_eq!(theme.user_message_bg.render().to_string(), "\x1b[40m");
    assert_eq!(theme.tool_success_bg.render().to_string(), "\x1b[40m");
    assert_eq!(theme.tool_error_bg.render().to_string(), "\x1b[40m");
}

#[test]
fn hex_color_parsing_valid_and_invalid() {
    assert_eq!(parse_color("#88c0d0"), Some(Color::Rgb(RgbColor(0x88, 0xc0, 0xd0))));
    assert_eq!(parse_color("#f0a"), Some(Color::Rgb(RgbColor(0xff, 0x00, 0xaa))));
    assert_eq!(parse_color("#xyz123"), None);
    assert_eq!(parse_color("#1234"), None);
    assert_eq!(parse_color("not_a_color"), None);
}

#[test]
fn named_ansi_color_parsing() {
    assert_eq!(parse_color("cyan"), Some(Color::Ansi(AnsiColor::Cyan)));
    assert_eq!(parse_color("bright_red"), Some(Color::Ansi(AnsiColor::BrightRed)));
    assert_eq!(parse_color("bright-blue"), Some(Color::Ansi(AnsiColor::BrightBlue)));
    assert_eq!(parse_color("gray"), Some(Color::Ansi(AnsiColor::BrightBlack)));
}

#[test]
fn theme_def_partial_override_falls_back_to_default() {
    let def = ThemeDef {
        prompt: Some("#88c0d0".into()),
        ..Default::default()
    };
    let theme = def.into_theme("custom");
    assert_eq!(theme.name, "custom");
    assert_eq!(theme.prompt.render().to_string(), "\x1b[38;2;136;192;208m");
    assert_eq!(theme.tool_ok.render().to_string(), "\x1b[32m");
}

#[test]
fn all_10_builtin_themes_load_and_have_metadata() {
    let registry = ThemeRegistry::default();
    let expected = [
        "default",
        "catppuccin",
        "nord",
        "tokyo-night",
        "dracula",
        "gruvbox",
        "monokai",
        "one-dark",
        "solarized-dark",
        "catppuccin-latte",
    ];
    for name in expected {
        assert!(registry.contains(name), "missing expected theme: {name}");
        let theme = registry.get(name).unwrap();
        assert_eq!(theme.name, name);
        let meta = registry.metadata(name).unwrap();
        assert!(!meta.description.is_empty());
    }
}

#[test]
fn default_theme_uses_only_ansi_colors() {
    let theme = Theme::default();
    assert!(theme.is_ansi());
    assert_eq!(theme.terminal_bg, None);
    assert_eq!(theme.terminal_fg, None);
    assert!(matches!(theme.prompt.get_fg_color(), Some(Color::Ansi(_))));
    assert!(matches!(theme.tool_header.get_fg_color(), Some(Color::Ansi(_))));
    assert!(matches!(theme.tool_ok.get_fg_color(), Some(Color::Ansi(_))));
    assert!(matches!(theme.tool_err.get_fg_color(), Some(Color::Ansi(_))));
    assert!(matches!(theme.highlight.get_fg_color(), Some(Color::Ansi(_))));
    assert!(matches!(theme.warning.get_fg_color(), Some(Color::Ansi(_))));
    assert!(matches!(theme.skill_tag.get_fg_color(), Some(Color::Ansi(_))));
    assert!(matches!(theme.user_message_bg.get_bg_color(), Some(Color::Ansi(_))));
    assert!(matches!(theme.tool_success_bg.get_bg_color(), Some(Color::Ansi(_))));
    assert!(matches!(theme.tool_error_bg.get_bg_color(), Some(Color::Ansi(_))));
}

#[test]
fn built_in_themes_use_only_hex_rgb_colors() {
    let registry = ThemeRegistry::default();
    for meta in registry.list() {
        if meta.name == "default" || meta.name == "ansi" {
            continue;
        }
        let theme = registry.get(&meta.name).unwrap();
        assert!(!theme.is_ansi(), "theme {} should not be ANSI", meta.name);
        assert!(theme.terminal_bg.is_some(), "{}: terminal_bg", meta.name);
        assert!(theme.terminal_fg.is_some(), "{}: terminal_fg", meta.name);
        assert!(
            matches!(theme.prompt.get_fg_color(), Some(Color::Rgb(_))),
            "{}: prompt",
            meta.name
        );
        assert!(
            matches!(theme.assistant.get_fg_color(), Some(Color::Rgb(_))),
            "{}: assistant",
            meta.name
        );
        assert!(
            matches!(theme.thinking.get_fg_color(), Some(Color::Rgb(_))),
            "{}: thinking",
            meta.name
        );
        assert!(
            matches!(theme.tool_header.get_fg_color(), Some(Color::Rgb(_))),
            "{}: tool_header",
            meta.name
        );
        assert!(
            matches!(theme.tool_ok.get_fg_color(), Some(Color::Rgb(_))),
            "{}: tool_ok",
            meta.name
        );
        assert!(
            matches!(theme.tool_err.get_fg_color(), Some(Color::Rgb(_))),
            "{}: tool_err",
            meta.name
        );
        assert!(
            matches!(theme.highlight.get_fg_color(), Some(Color::Rgb(_))),
            "{}: highlight",
            meta.name
        );
        assert!(
            matches!(theme.code_inline.get_fg_color(), Some(Color::Rgb(_))),
            "{}: code_inline",
            meta.name
        );
        assert!(
            matches!(theme.heading_h1.get_fg_color(), Some(Color::Rgb(_))),
            "{}: heading_h1",
            meta.name
        );
        assert!(
            matches!(theme.heading_h2.get_fg_color(), Some(Color::Rgb(_))),
            "{}: heading_h2",
            meta.name
        );
        assert!(
            matches!(theme.heading_h3.get_fg_color(), Some(Color::Rgb(_))),
            "{}: heading_h3",
            meta.name
        );
        assert!(
            matches!(theme.dimmed.get_fg_color(), Some(Color::Rgb(_))),
            "{}: dimmed",
            meta.name
        );
        assert!(
            matches!(theme.warning.get_fg_color(), Some(Color::Rgb(_))),
            "{}: warning",
            meta.name
        );
        assert!(
            matches!(theme.skill_tag.get_fg_color(), Some(Color::Rgb(_))),
            "{}: skill_tag",
            meta.name
        );
        assert!(
            matches!(theme.user_message_bg.get_bg_color(), Some(Color::Rgb(_))),
            "{}: user_message_bg",
            meta.name
        );
        assert!(
            matches!(theme.tool_success_bg.get_bg_color(), Some(Color::Rgb(_))),
            "{}: tool_success_bg",
            meta.name
        );
        assert!(
            matches!(theme.tool_error_bg.get_bg_color(), Some(Color::Rgb(_))),
            "{}: tool_error_bg",
            meta.name
        );
    }
}

#[test]
fn registry_aliases_and_listing() {
    let registry = ThemeRegistry::default();
    assert!(registry.contains("ansi"));
    assert!(registry.contains("catppuccin-mocha"));

    let list = registry.list();
    assert_eq!(list.len(), 10);
    assert_eq!(list[0].name, "default");
    assert_eq!(list[9].name, "catppuccin-latte");
    assert!(list[9].is_light);
}

#[test]
fn registry_loads_custom_themes_from_directory() {
    let temp_dir = std::env::temp_dir().join(format!("rho_theme_test_{}", uuid::Uuid::new_v4()));
    let themes_dir = temp_dir.join("themes");
    std::fs::create_dir_all(&themes_dir).unwrap();

    let theme_content = r##"
name = "my-custom"
description = "My test custom theme"
is_light = false
prompt = "#ff00ff"
highlight = "#00ffff"
"##;
    std::fs::write(themes_dir.join("my-custom.toml"), theme_content).unwrap();

    let registry = ThemeRegistry::new(Some(&temp_dir));
    assert!(registry.contains("my-custom"));
    let meta = registry.metadata("my-custom").unwrap();
    assert_eq!(meta.description, "My test custom theme");
    assert!(meta.is_custom);

    let theme = registry.get("my-custom").unwrap();
    assert_eq!(theme.prompt.render().to_string(), "\x1b[38;2;255;0;255m");

    let _ = std::fs::remove_dir_all(&temp_dir);
}
