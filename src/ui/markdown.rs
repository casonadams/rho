use crate::ui::theme::Theme;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{ContentArrangement, Table};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

#[derive(Default)]
pub struct MarkdownRenderer {
    in_code_block: bool,
    in_mermaid_block: bool,
    code_lang: Option<String>,
    current_line: String,
    table_lines: Vec<String>,
    mermaid_lines: Vec<String>,
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn render_token(&mut self, token: &str, theme: &Theme) -> String {
        let mut out = String::new();
        let mut remaining = token;

        while let Some(pos) = remaining.find('\n') {
            let chunk = &remaining[..pos];
            self.current_line.push_str(chunk);
            let line = std::mem::take(&mut self.current_line);
            out.push_str(&self.process_line(&line, theme));
            remaining = &remaining[pos + 1..];
        }

        if !remaining.is_empty() {
            self.current_line.push_str(remaining);
        }

        out
    }

    pub fn flush(&mut self, theme: &Theme) -> String {
        let mut out = String::new();
        if !self.table_lines.is_empty() {
            let lines = std::mem::take(&mut self.table_lines);
            out.push_str(&render_markdown_table(&lines, theme));
        }
        if self.in_mermaid_block && !self.mermaid_lines.is_empty() {
            let lines = std::mem::take(&mut self.mermaid_lines);
            self.in_mermaid_block = false;
            out.push_str(&render_mermaid_block(&lines.join("\n"), theme));
        }
        if !self.current_line.is_empty() {
            let line = std::mem::take(&mut self.current_line);
            out.push_str(&self.process_line(&line, theme));
        }
        out
    }

    fn process_line(&mut self, line: &str, theme: &Theme) -> String {
        let trimmed = line.trim();

        if let Some(rendered) = self.check_mermaid_toggle(trimmed, theme) {
            return rendered;
        }

        if self.in_mermaid_block {
            self.mermaid_lines.push(line.to_string());
            return String::new();
        }

        if is_table_line(trimmed) {
            self.table_lines.push(line.to_string());
            return String::new();
        }

        let mut out = String::new();
        if !self.table_lines.is_empty() {
            let lines = std::mem::take(&mut self.table_lines);
            out.push_str(&render_markdown_table(&lines, theme));
        }

        out.push_str(&self.render_line(line, theme));
        out.push('\n');
        out
    }

    fn check_mermaid_toggle(&mut self, trimmed: &str, theme: &Theme) -> Option<String> {
        if !trimmed.starts_with("```") {
            return None;
        }
        let tag = trimmed.trim_start_matches('`').trim();
        if self.in_mermaid_block {
            self.in_mermaid_block = false;
            let src = std::mem::take(&mut self.mermaid_lines).join("\n");
            Some(render_mermaid_block(&src, theme))
        } else if tag.eq_ignore_ascii_case("mermaid") {
            self.in_mermaid_block = true;
            self.mermaid_lines.clear();
            Some(String::new())
        } else {
            None
        }
    }

    pub fn render_line(&mut self, line: &str, theme: &Theme) -> String {
        let trimmed = line.trim();

        if trimmed.starts_with("```") {
            return self.toggle_code_fence(trimmed, theme);
        }

        if self.in_code_block {
            return highlight_code_line(line, self.code_lang.as_deref(), theme);
        }

        if let Some(header) = self.render_header(line, theme) {
            return header;
        }

        if let Some(list_item) = self.render_list_item(line, theme) {
            return list_item;
        }

        if let Some(quote) = line.strip_prefix("> ") {
            let d = theme.dimmed;
            let formatted = render_inline_elements(quote, theme);
            return format!("{d}│{d:#} {formatted}");
        }

        render_inline_elements(line, theme)
    }

    fn render_header(&self, line: &str, theme: &Theme) -> Option<String> {
        if let Some(rest) = line.strip_prefix("### ") {
            let h = theme.heading_h3;
            let formatted = render_inline_elements(rest, theme);
            return Some(format!("\n{h}### {formatted}{h:#}"));
        }
        if let Some(rest) = line.strip_prefix("## ") {
            let p = theme.heading_h2;
            let formatted = render_inline_elements(rest, theme);
            return Some(format!("\n{p}## {formatted}{p:#}"));
        }
        if let Some(rest) = line.strip_prefix("# ") {
            let hl = theme.heading_h1;
            let formatted = render_inline_elements(rest, theme);
            return Some(format!("\n{hl}# {formatted}{hl:#}"));
        }
        None
    }

    fn render_list_item(&self, line: &str, theme: &Theme) -> Option<String> {
        if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            let p = theme.prompt;
            let formatted = render_inline_elements(rest, theme);
            return Some(format!("{p}•{p:#} {formatted}"));
        }
        if let Some(caps) = regex::Regex::new(r"^(\d+\.)\s+(.*)$").ok()?.captures(line) {
            let num = &caps[1];
            let rest = &caps[2];
            let p = theme.prompt;
            let formatted = render_inline_elements(rest, theme);
            return Some(format!("{p}{num}{p:#} {formatted}"));
        }
        None
    }

    fn toggle_code_fence(&mut self, trimmed: &str, theme: &Theme) -> String {
        let tag = trimmed.trim_start_matches('`').trim();
        let d = theme.dimmed;
        if self.in_code_block {
            self.in_code_block = false;
            self.code_lang = None;
            format!("{d}```{d:#}")
        } else {
            self.in_code_block = true;
            self.code_lang = (!tag.is_empty()).then(|| tag.to_string());
            if let Some(ref l) = self.code_lang {
                format!("{d}```{l}{d:#}")
            } else {
                format!("{d}```{d:#}")
            }
        }
    }
}

pub fn highlight_code_line(line: &str, lang: Option<&str>, theme: &Theme) -> String {
    let ss = &*SYNTAX_SET;
    let ts = &*THEME_SET;
    let syntax = lang
        .and_then(|l| ss.find_syntax_by_token(l))
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

pub fn render_inline_elements(text: &str, theme: &Theme) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(text, options);
    let mut out = String::new();

    let bold_style = anstyle::Style::new().bold();
    let italic_style = anstyle::Style::new().italic();
    let strike_style = anstyle::Style::new().strikethrough();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Strong => out.push_str(&bold_style.render().to_string()),
                Tag::Emphasis => out.push_str(&italic_style.render().to_string()),
                Tag::Strikethrough => out.push_str(&strike_style.render().to_string()),
                Tag::Link { .. } => out.push_str(&theme.highlight.render().to_string()),
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Strong => out.push_str(&bold_style.render_reset().to_string()),
                TagEnd::Emphasis => out.push_str(&italic_style.render_reset().to_string()),
                TagEnd::Strikethrough => out.push_str(&strike_style.render_reset().to_string()),
                TagEnd::Link => out.push_str(&theme.highlight.render_reset().to_string()),
                _ => {}
            },
            Event::Text(t) => out.push_str(&t),
            Event::Code(c) => {
                let h = theme.code_inline;
                out.push_str(&format!("{h}`{c}`{h:#}"));
            }
            Event::SoftBreak => out.push(' '),
            Event::HardBreak => out.push('\n'),
            _ => {}
        }
    }

    let trailing_spaces = text.len() - text.trim_end_matches(' ').len();
    if trailing_spaces > 0 && !out.ends_with(' ') {
        for _ in 0..trailing_spaces {
            out.push(' ');
        }
    }

    out
}

pub fn render_mermaid_block(source: &str, theme: &Theme) -> String {
    let header = theme.tool_header;
    let dim = theme.dimmed;

    let mut out = format!("\n{header}[mermaid diagram]{header:#}\n");
    match meraid::render(source, meraid::ThemeType::default()) {
        Ok(rendered) => {
            for line in rendered.lines() {
                out.push_str(&format!("{line}\n"));
            }
        }
        Err(_) => {
            for line in source.lines() {
                out.push_str(&format!("{dim}│{dim:#} {line}\n"));
            }
        }
    }
    out.push('\n');
    out
}

pub fn is_table_line(trimmed: &str) -> bool {
    (trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() >= 2) || is_table_divider(trimmed)
}

pub fn is_table_divider(line: &str) -> bool {
    let stripped: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    stripped.starts_with('|')
        && stripped.ends_with('|')
        && stripped.len() >= 3
        && stripped.chars().all(|c| c == '|' || c == '-' || c == ':')
}

pub fn render_markdown_table(lines: &[String], _theme: &Theme) -> String {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_content_arrangement(ContentArrangement::Dynamic);

    if let Ok((cols, _)) = crossterm::terminal::size() {
        let max_w = cols.saturating_sub(4).max(40);
        table.set_width(max_w);
    }

    let mut is_header = true;
    for line in lines {
        let trimmed = line.trim();
        if is_table_divider(trimmed) {
            is_header = false;
            continue;
        }

        let cells: Vec<String> = trimmed
            .trim_matches('|')
            .split('|')
            .map(|s| strip_markdown_decorations(s.trim()))
            .collect();

        if is_header {
            table.set_header(cells);
            is_header = false;
        } else {
            table.add_row(cells);
        }
    }

    format!("\n{table}\n")
}

pub fn strip_markdown_decorations(s: &str) -> String {
    s.replace("**", "").replace(['*', '`'], "").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_rendering() {
        let theme = Theme::default();
        let mut md = MarkdownRenderer::new();

        let chunk = "| Category | Details |\n|---|---|\n| Architecture | Linear Loop |\n\n";
        let out = md.render_token(chunk, &theme);
        assert!(out.contains("Category"));
        assert!(out.contains("Details"));
        assert!(out.contains("Architecture"));
        assert!(out.contains("Linear Loop"));
        assert!(out.contains('┌') || out.contains('+') || out.contains('-') || out.contains('│'));
    }

    #[test]
    fn test_stream_line_by_line() {
        let theme = Theme::default();
        let mut md = MarkdownRenderer::new();

        let token1 = md.render_token("Hello ", &theme);
        assert_eq!(token1, "");

        let token2 = md.render_token("world\n", &theme);
        assert_eq!(token2, "Hello world\n");
    }

    #[test]
    fn test_mermaid_rendering() {
        let theme = Theme::default();
        let mut md = MarkdownRenderer::new();

        let chunk = "```mermaid\ngraph TD\n  A[Start] --> B[End]\n```\n\n";
        let out = md.render_token(chunk, &theme);
        assert!(out.contains("mermaid diagram"));
    }

    #[test]
    fn test_bold_and_italic_rendering() {
        let theme = Theme::default();
        let res = render_inline_elements("This is **important** and *italic* text", &theme);
        assert!(!res.contains("**"));
        assert!(res.contains("important"));
        assert!(res.contains("\x1b[1m"));
        assert!(res.contains("italic"));
        assert!(res.contains("\x1b[3m"));
    }

    #[test]
    fn test_code_block_has_no_background_color_patches() {
        let theme = Theme::default();
        let highlighted = highlight_code_line("let x = 42;", Some("rust"), &theme);
        // 24-bit background color escape is \x1b[48;2;...
        assert!(!highlighted.contains("\x1b[48;2;"));
        assert!(highlighted.contains("let"));
        assert!(highlighted.contains("42"));
    }

    #[test]
    fn test_code_block_fences_open_and_close() {
        let theme = Theme::default();
        let mut md = MarkdownRenderer::new();

        let l1 = md.render_line("```rust", &theme);
        assert!(l1.contains("```rust"));

        let l2 = md.render_line("fn main() {}", &theme);
        assert!(l2.contains("fn"));

        let l3 = md.render_line("```", &theme);
        assert!(l3.contains("```"));
    }

    #[test]
    fn test_header_rendering() {
        let theme = Theme::default();
        let mut md = MarkdownRenderer::new();

        let h1 = md.render_line("# Main Title", &theme);
        assert!(h1.contains("Main Title"));

        let bullet = md.render_line("- first item", &theme);
        assert!(bullet.contains("first item"));
        assert!(bullet.contains('•'));

        let num = md.render_line("1. First step", &theme);
        assert!(num.contains("1."));
        assert!(num.contains("First step"));
    }

    #[test]
    fn test_flush_emits_newline_when_line_uncompleted() {
        let theme = Theme::default();
        let mut md = MarkdownRenderer::new();

        let token = md.render_token("Hello world", &theme);
        assert_eq!(token, "");

        let flushed = md.flush(&theme);
        assert_eq!(flushed, "Hello world\n");

        let second_flush = md.flush(&theme);
        assert_eq!(second_flush, "");
    }

    #[test]
    fn test_flush_does_not_emit_redundant_newline_when_already_terminated() {
        let theme = Theme::default();
        let mut md = MarkdownRenderer::new();

        let token = md.render_token("Hello world\n", &theme);
        assert_eq!(token, "Hello world\n");

        let flushed = md.flush(&theme);
        assert_eq!(flushed, "");
    }

    #[test]
    fn test_math_and_wildcard_asterisks_not_corrupted() {
        let theme = Theme::default();
        let res = render_inline_elements("formula: a * b * c and glob: *.rs", &theme);
        assert!(res.contains("a * b * c"));
        assert!(res.contains("*.rs"));
        assert!(!res.contains("\x1b[3m"));
    }
}
