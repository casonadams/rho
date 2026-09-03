//! Code-block syntax highlighting via `syntect`.
//!
//! Reduces `syntect`'s 24-bit color output to ANSI-16 escape codes so the result
//! composes with the rest of our ANSI-styled output.

use crate::ui::theme::Theme;
use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

pub fn highlight_code_line(line: &str, lang: Option<&str>, theme: &Theme) -> String {
    let ss = &*SYNTAX_SET;
    let ts = &*THEME_SET;
    let syntax = lang
        .and_then(|l| ss.find_syntax_by_token(l).or_else(|| ss.find_syntax_by_extension(l)))
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let syn_theme = &ts.themes["base16-ocean.dark"];
    let mut highlighter = HighlightLines::new(syntax, syn_theme);
    if let Ok(ranges) = highlighter.highlight_line(line, ss) {
        let mut out = String::new();
        for (style, text) in ranges {
            let ansi = syntect_color_to_ansi16(style.foreground);
            out.push_str(ansi);
            out.push_str(text);
        }
        out.push_str("\x1b[0m");
        out
    } else {
        let d = theme.dimmed;
        format!("{d}{line}{d:#}")
    }
}

fn syntect_color_to_ansi16(color: syntect::highlighting::Color) -> &'static str {
    let (r, g, b) = (color.r, color.g, color.b);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);

    if max.saturating_sub(min) < 20 {
        return if max < 140 { "\x1b[90m" } else { "\x1b[37m" };
    }

    if r >= g && r >= b {
        dominant_red_ansi(g, b)
    } else if g >= r && g >= b {
        dominant_green_ansi(b)
    } else {
        dominant_blue_ansi(r, g)
    }
}

fn dominant_red_ansi(g: u8, b: u8) -> &'static str {
    if g > 130 {
        "\x1b[33m"
    } else if b > 130 {
        "\x1b[35m"
    } else {
        "\x1b[31m"
    }
}

fn dominant_green_ansi(b: u8) -> &'static str {
    if b > 130 { "\x1b[36m" } else { "\x1b[32m" }
}

fn dominant_blue_ansi(r: u8, g: u8) -> &'static str {
    if r > 130 {
        "\x1b[35m"
    } else if g > 130 {
        "\x1b[36m"
    } else {
        "\x1b[34m"
    }
}
