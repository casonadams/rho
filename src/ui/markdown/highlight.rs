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

fn resolve_highlighter<'a>(lang: Option<&str>, is_light: bool) -> HighlightLines<'a> {
    let ss = &*SYNTAX_SET;
    let ts = &*THEME_SET;
    let syntax = lang
        .and_then(|l| ss.find_syntax_by_token(l).or_else(|| ss.find_syntax_by_extension(l)))
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let syn_theme = if is_light {
        &ts.themes["base16-ocean.light"]
    } else {
        &ts.themes["base16-ocean.dark"]
    };
    HighlightLines::new(syntax, syn_theme)
}

fn format_highlighted_ranges(ranges: &[(syntect::highlighting::Style, &str)]) -> String {
    let mut out = String::new();
    for (style, text) in ranges {
        out.push_str(syntect_color_to_ansi16(style.foreground));
        out.push_str(text);
    }
    out.push_str("\x1b[0m");
    out
}

pub struct CodeHighlighter<'a> {
    highlighter: Option<HighlightLines<'a>>,
}

impl<'a> CodeHighlighter<'a> {
    pub fn new(lang: Option<&str>, theme: &Theme) -> Self {
        let highlighter = Some(resolve_highlighter(lang, theme.is_light));
        Self { highlighter }
    }

    pub fn highlight_line(&mut self, line: &str, theme: &Theme) -> String {
        if let Some(ref mut h) = self.highlighter
            && let Ok(ranges) = h.highlight_line(line, &SYNTAX_SET)
        {
            format_highlighted_ranges(&ranges)
        } else {
            let d = theme.dimmed;
            format!("{d}{line}{d:#}")
        }
    }
}

pub fn highlight_code_line(line: &str, lang: Option<&str>, theme: &Theme) -> String {
    CodeHighlighter::new(lang, theme).highlight_line(line, theme)
}

fn grayscale_ansi(lightness: u16) -> &'static str {
    match lightness {
        0..=80 => "\x1b[30m",
        81..=200 => "\x1b[90m",
        201..=400 => "\x1b[37m",
        _ => "\x1b[97m",
    }
}

fn yellow_or_magenta(g: u8, b: u8, is_bright: bool) -> Option<&'static str> {
    if g.saturating_sub(b) > 30 {
        Some(if is_bright { "\x1b[93m" } else { "\x1b[33m" })
    } else if b.saturating_sub(g) > 30 {
        Some(if is_bright { "\x1b[95m" } else { "\x1b[35m" })
    } else {
        None
    }
}

fn red_dominant_ansi(g: u8, b: u8, is_bright: bool) -> &'static str {
    if let Some(ansi) = yellow_or_magenta(g, b, is_bright) {
        ansi
    } else if is_bright {
        "\x1b[91m"
    } else {
        "\x1b[31m"
    }
}

fn green_dominant_ansi(r: u8, b: u8, is_bright: bool) -> &'static str {
    if b > r && (b - r) > 30 {
        if is_bright { "\x1b[96m" } else { "\x1b[36m" }
    } else if is_bright {
        "\x1b[92m"
    } else {
        "\x1b[32m"
    }
}

fn syntect_color_to_ansi16(color: syntect::highlighting::Color) -> &'static str {
    let (r, g, b) = (color.r, color.g, color.b);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = u16::from(max) + u16::from(min);
    if max - min < 20 {
        return grayscale_ansi(lightness);
    }
    let is_bright = lightness > 256;
    if r >= g && r >= b {
        red_dominant_ansi(g, b, is_bright)
    } else if g >= r && g >= b {
        green_dominant_ansi(r, b, is_bright)
    } else if is_bright {
        "\x1b[94m"
    } else {
        "\x1b[34m"
    }
}
