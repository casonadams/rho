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
        let (r, g, b) = (style.foreground.r, style.foreground.g, style.foreground.b);
        out.push_str(&format!("\x1b[38;2;{r};{g};{b}m{text}"));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> syntect::highlighting::Color {
        syntect::highlighting::Color { r, g, b, a: 0xFF }
    }

    #[test]
    fn truecolor_rgb_ranges_format_correctly() {
        let ranges = [(
            syntect::highlighting::Style {
                foreground: rgb(255, 120, 50),
                background: rgb(0, 0, 0),
                font_style: syntect::highlighting::FontStyle::empty(),
            },
            "fn main()",
        )];
        let out = format_highlighted_ranges(&ranges);
        assert_eq!(out, "\x1b[38;2;255;120;50mfn main()\x1b[0m");
    }
}
