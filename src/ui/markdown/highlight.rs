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
    lang: Option<String>,
    highlighter: HighlightLines<'a>,
}

impl<'a> CodeHighlighter<'a> {
    pub fn new(lang: Option<&str>, theme: &Theme) -> Self {
        Self {
            lang: lang.map(str::to_string),
            highlighter: resolve_highlighter(lang, theme.is_light),
        }
    }

    pub fn lang(&self) -> Option<&str> {
        self.lang.as_deref()
    }

    pub fn highlight_line(&mut self, line: &str, theme: &Theme) -> String {
        match self.highlighter.highlight_line(line, &SYNTAX_SET) {
            Ok(ranges) => format_highlighted_ranges(&ranges),
            Err(_) => {
                let d = theme.dimmed;
                format!("{d}{line}{d:#}")
            }
        }
    }
}

pub fn highlight_code_line(line: &str, lang: Option<&str>, theme: &Theme) -> String {
    CodeHighlighter::new(lang, theme).highlight_line(line, theme)
}

const ANSI16_CODES: [&str; 16] = [
    "\x1b[30m", "\x1b[31m", "\x1b[32m", "\x1b[33m", "\x1b[34m", "\x1b[35m", "\x1b[36m", "\x1b[37m", "\x1b[90m",
    "\x1b[91m", "\x1b[92m", "\x1b[93m", "\x1b[94m", "\x1b[95m", "\x1b[96m", "\x1b[97m",
];

fn ansi16(code: u8, is_bright: bool) -> &'static str {
    ANSI16_CODES[usize::from(if is_bright { 8 + code } else { code })]
}

fn grayscale_ansi(lightness: u16) -> &'static str {
    match lightness {
        0..=80 => ansi16(0, false),
        81..=200 => ansi16(0, true),
        201..=400 => ansi16(7, false),
        _ => ansi16(7, true),
    }
}

fn yellow_or_magenta(g: u8, b: u8, is_bright: bool) -> Option<&'static str> {
    if g.saturating_sub(b) > 30 {
        Some(ansi16(3, is_bright))
    } else if b.saturating_sub(g) > 30 {
        Some(ansi16(5, is_bright))
    } else {
        None
    }
}

fn red_dominant_ansi(g: u8, b: u8, is_bright: bool) -> &'static str {
    if let Some(ansi) = yellow_or_magenta(g, b, is_bright) {
        return ansi;
    }
    ansi16(1, is_bright)
}

fn green_dominant_ansi(r: u8, b: u8, is_bright: bool) -> &'static str {
    if b > r && (b - r) > 30 {
        ansi16(6, is_bright)
    } else {
        ansi16(2, is_bright)
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
    } else {
        ansi16(4, is_bright)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> syntect::highlighting::Color {
        syntect::highlighting::Color { r, g, b, a: 0xFF }
    }

    #[test]
    fn ansi16_mapping_matches_existing_escapes() {
        let cases = [
            (rgb(0, 0, 0), "\x1b[30m"),
            (rgb(40, 40, 40), "\x1b[30m"),
            (rgb(41, 41, 41), "\x1b[90m"),
            (rgb(100, 100, 100), "\x1b[90m"),
            (rgb(101, 101, 101), "\x1b[37m"),
            (rgb(128, 128, 128), "\x1b[37m"),
            (rgb(200, 200, 200), "\x1b[37m"),
            (rgb(201, 201, 201), "\x1b[97m"),
            (rgb(255, 255, 255), "\x1b[97m"),
            (rgb(255, 0, 0), "\x1b[31m"),
            (rgb(255, 120, 120), "\x1b[91m"),
            (rgb(0, 200, 0), "\x1b[32m"),
            (rgb(0, 255, 0), "\x1b[32m"),
            (rgb(0, 255, 100), "\x1b[36m"),
            (rgb(100, 255, 255), "\x1b[96m"),
            (rgb(0, 255, 255), "\x1b[36m"),
            (rgb(0, 0, 255), "\x1b[34m"),
            (rgb(100, 100, 255), "\x1b[94m"),
            (rgb(255, 255, 0), "\x1b[33m"),
            (rgb(255, 255, 100), "\x1b[93m"),
            (rgb(255, 0, 255), "\x1b[35m"),
            (rgb(255, 100, 255), "\x1b[95m"),
        ];
        for (color, expected) in cases {
            assert_eq!(syntect_color_to_ansi16(color), expected, "for rgb {color:?}");
        }
    }
}
