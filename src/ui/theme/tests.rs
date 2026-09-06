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

fn assert_all_ansi_fg(styles: &[Style]) {
    for s in styles {
        assert!(matches!(s.get_fg_color(), Some(Color::Ansi(_))));
    }
}

fn assert_all_ansi_bg(styles: &[Style]) {
    for s in styles {
        assert!(matches!(s.get_bg_color(), Some(Color::Ansi(_))));
    }
}

#[test]
fn default_theme_uses_only_ansi_colors() {
    let theme = Theme::default();
    assert!(theme.is_ansi());
    assert_all_ansi_fg(&[
        theme.prompt,
        theme.tool_header,
        theme.tool_ok,
        theme.tool_err,
        theme.highlight,
        theme.warning,
        theme.skill_tag,
    ]);
    assert_all_ansi_bg(&[theme.user_message_bg, theme.tool_success_bg, theme.tool_error_bg]);
}

#[test]
fn hex_color_parsing_valid() {
    assert_eq!(parse_color("#88c0d0"), Some(Color::Rgb(RgbColor(0x88, 0xc0, 0xd0))));
    assert_eq!(parse_color("#f0a"), Some(Color::Rgb(RgbColor(0xff, 0x00, 0xaa))));
}

#[test]
fn hex_color_parsing_invalid() {
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

fn assert_partial_palette_theme(theme: &Theme) {
    let actual1 = (theme.name.as_str(), theme.is_ansi(), theme.tool_ok.render().to_string());
    assert_eq!(actual1, ("custom", false, "\x1b[38;2;166;227;161m".to_string()));
    let actual2 = (
        theme.tool_err.render().to_string(),
        theme.user_message_bg.render().to_string(),
        theme.prompt.render().to_string(),
    );
    assert_eq!(
        actual2,
        (
            "\x1b[38;2;243;139;168m".to_string(),
            "\x1b[40m".to_string(),
            Theme::default().prompt.render().to_string()
        )
    );
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
    assert_partial_palette_theme(&theme);
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

fn assert_styles_rgb_fg(styles: &[Style]) {
    for s in styles {
        assert!(matches!(s.get_fg_color(), Some(Color::Rgb(_))));
    }
}

fn assert_styles_rgb_bg(styles: &[Style]) {
    for s in styles {
        assert!(matches!(s.get_bg_color(), Some(Color::Rgb(_))));
    }
}

fn assert_theme_rgb(theme: &Theme) {
    assert_styles_rgb_fg(&[
        theme.prompt,
        theme.thinking,
        theme.tool_header,
        theme.tool_ok,
        theme.tool_err,
        theme.highlight,
        theme.code_inline,
        theme.heading_h3,
        theme.dimmed,
        theme.warning,
        theme.skill_tag,
    ]);
    assert_styles_rgb_bg(&[theme.user_message_bg, theme.tool_success_bg, theme.tool_error_bg]);
}

#[test]
fn built_in_themes_use_only_hex_rgb_colors() {
    let registry = ThemeRegistry::default();
    let themes = registry
        .list()
        .into_iter()
        .filter(|m| m.name != "default" && m.name != "ansi");
    for meta in themes {
        let theme = registry.get(&meta.name).unwrap();
        assert!(!theme.is_ansi());
        assert_theme_rgb(theme);
    }
}

#[test]
fn registry_aliases() {
    let registry = ThemeRegistry::default();
    assert!(registry.contains("ansi") && registry.contains("catppuccin-mocha"));
}

#[test]
fn registry_listing() {
    let registry = ThemeRegistry::default();
    let list = registry.list();
    assert_eq!(
        (list.len(), list[0].name.as_str(), list[9].name.as_str()),
        (10, "default", "catppuccin-latte")
    );
    assert!(list[9].is_light);
}

fn write_custom_theme_file(themes_dir: &std::path::Path) {
    let content = "name = \"my-custom\"\ndescription = \"My test custom theme\"\nis_light = false\nbackground = \"#1a1b26\"\nforeground = \"#c0caf5\"\nblack = \"#15161e\"\nred = \"#f7768e\"\ngreen = \"#9ece6a\"\nyellow = \"#e0af68\"\nblue = \"#7aa2f7\"\nmagenta = \"#bb9af7\"\ncyan = \"#7dcfff\"\nwhite = \"#a9b1d6\"\nbright_black = \"#414868\"\nbright_red = \"#f7768e\"\nbright_green = \"#9ece6a\"\nbright_yellow = \"#e0af68\"\nbright_blue = \"#7aa2f7\"\nbright_magenta = \"#bb9af7\"\nbright_cyan = \"#2ac3de\"\nbright_white = \"#c0caf5\"\n";
    std::fs::write(themes_dir.join("my-custom.toml"), content).unwrap();
}

fn assert_custom_theme_loaded(registry: &ThemeRegistry) {
    assert!(registry.contains("my-custom"));
    let meta = registry.metadata("my-custom").unwrap();
    assert_eq!(
        (meta.description.as_str(), meta.is_custom),
        ("My test custom theme", true)
    );
    let theme = registry.get("my-custom").unwrap();
    assert_eq!(theme.tool_ok.render().to_string(), "\x1b[38;2;158;206;106m");
    assert_eq!(theme.user_message_bg.render().to_string(), "\x1b[48;2;21;22;30m");
}

#[test]
fn registry_loads_custom_themes_from_directory() {
    let temp_dir = std::env::temp_dir().join(format!("rho_theme_test_{}", uuid::Uuid::new_v4()));
    let themes_dir = temp_dir.join("themes");
    std::fs::create_dir_all(&themes_dir).unwrap();
    write_custom_theme_file(&themes_dir);

    let registry = ThemeRegistry::new(Some(&temp_dir));
    assert_custom_theme_loaded(&registry);
    let _ = std::fs::remove_dir_all(temp_dir);
}
