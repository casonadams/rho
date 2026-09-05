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
fn default_theme_uses_only_ansi_colors() {
    let theme = Theme::default();
    assert!(theme.is_ansi());
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
fn theme_def_partial_palette_maps_available_roles() {
    let def = ThemeDef {
        background: Some("#1e1e2e".into()),
        foreground: Some("#cdd6f4".into()),
        green: Some("#a6e3a1".into()),
        red: Some("#f38ba8".into()),
        ..Default::default()
    };
    let theme = def.into_theme("custom");
    assert_eq!(theme.name, "custom");
    assert!(!theme.is_ansi());
    assert_eq!(theme.tool_ok.render().to_string(), "\x1b[38;2;166;227;161m");
    assert_eq!(theme.tool_err.render().to_string(), "\x1b[38;2;243;139;168m");
    assert_eq!(
        theme.user_message_bg.render().to_string(),
        "\x1b[40m",
        "block backgrounds use the color0 surface, falling back to ANSI black"
    );
    assert_eq!(
        theme.prompt.render().to_string(),
        Theme::default().prompt.render().to_string(),
        "unmapped roles keep the default ANSI styling"
    );
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
fn built_in_themes_use_only_hex_rgb_colors() {
    let registry = ThemeRegistry::default();
    for meta in registry.list() {
        if meta.name == "default" || meta.name == "ansi" {
            continue;
        }
        let theme = registry.get(&meta.name).unwrap();
        assert!(!theme.is_ansi(), "theme {} should not be ANSI", meta.name);
        for (label, style) in [
            ("prompt", theme.prompt),
            ("thinking", theme.thinking),
            ("tool_header", theme.tool_header),
            ("tool_ok", theme.tool_ok),
            ("tool_err", theme.tool_err),
            ("highlight", theme.highlight),
            ("code_inline", theme.code_inline),
            ("heading_h3", theme.heading_h3),
            ("dimmed", theme.dimmed),
            ("warning", theme.warning),
            ("skill_tag", theme.skill_tag),
        ] {
            assert!(
                matches!(style.get_fg_color(), Some(Color::Rgb(_))),
                "{}: {label}",
                meta.name
            );
        }
        for (label, style) in [
            ("user_message_bg", theme.user_message_bg),
            ("tool_success_bg", theme.tool_success_bg),
            ("tool_error_bg", theme.tool_error_bg),
        ] {
            assert!(
                matches!(style.get_bg_color(), Some(Color::Rgb(_))),
                "{}: {label}",
                meta.name
            );
        }
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

background = "#1a1b26"
foreground = "#c0caf5"

black = "#15161e"
red = "#f7768e"
green = "#9ece6a"
yellow = "#e0af68"
blue = "#7aa2f7"
magenta = "#bb9af7"
cyan = "#7dcfff"
white = "#a9b1d6"

bright_black = "#414868"
bright_red = "#f7768e"
bright_green = "#9ece6a"
bright_yellow = "#e0af68"
bright_blue = "#7aa2f7"
bright_magenta = "#bb9af7"
bright_cyan = "#2ac3de"
bright_white = "#c0caf5"
"##;
    std::fs::write(themes_dir.join("my-custom.toml"), theme_content).unwrap();

    let registry = ThemeRegistry::new(Some(&temp_dir));
    assert!(registry.contains("my-custom"));
    let meta = registry.metadata("my-custom").unwrap();
    assert_eq!(meta.description, "My test custom theme");
    assert!(meta.is_custom);

    let theme = registry.get("my-custom").unwrap();
    assert_eq!(theme.tool_ok.render().to_string(), "\x1b[38;2;158;206;106m");
    assert_eq!(theme.user_message_bg.render().to_string(), "\x1b[48;2;21;22;30m");

    let _ = std::fs::remove_dir_all(&temp_dir);
}
