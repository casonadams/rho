//! Core `MarkdownRenderer` state machine.
//!
//! Owns the streaming state (current line, open code/mermaid blocks, table buffer)
//! and dispatches each input line to the appropriate handler.

use super::elements::{is_table_line, render_inline_elements, render_markdown_table, render_mermaid_block};
use super::highlight::highlight_code_line;
use crate::ui::theme::Theme;

#[derive(Default)]
pub struct MarkdownRenderer {
    in_code_block: bool,
    in_mermaid_block: bool,
    code_lang: Option<String>,
    current_line: String,
    emitted_on_current_line: bool,
    table_lines: Vec<String>,
    mermaid_lines: Vec<String>,
    in_bold: bool,
    in_italic: bool,
    in_code: bool,
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

            if self.emitted_on_current_line {
                out.push('\n');
                self.current_line.clear();
                self.emitted_on_current_line = false;
            } else {
                let line = std::mem::take(&mut self.current_line);
                out.push_str(&self.process_line(&line, theme));
            }

            remaining = &remaining[pos + 1..];
        }

        if !remaining.is_empty() {
            out.push_str(&self.handle_trailing_chunk(remaining, theme));
        }

        out
    }

    fn handle_trailing_chunk(&mut self, remaining: &str, theme: &Theme) -> String {
        self.current_line.push_str(remaining);

        if self.should_buffer_current_line() {
            return String::new();
        }

        self.emitted_on_current_line = true;
        self.render_inline_token(remaining, theme)
    }

    fn should_buffer_current_line(&self) -> bool {
        if self.in_code_block || self.in_mermaid_block || is_table_line(self.current_line.trim()) {
            return true;
        }

        let trimmed = self.current_line.trim_start();
        trimmed.starts_with('#')
            || trimmed.starts_with("```")
            || trimmed.starts_with("> ")
            || trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || (trimmed.starts_with('|') && trimmed.ends_with('|'))
    }

    fn render_inline_token(&mut self, token: &str, theme: &Theme) -> String {
        let mut out = String::new();
        let bold_style = anstyle::Style::new().bold();
        let italic_style = anstyle::Style::new().italic();

        let chars: Vec<char> = token.chars().collect();
        let len = chars.len();
        let mut i = 0;

        while i < len {
            if chars[i] == '`' {
                if self.in_code {
                    out.push('`');
                    out.push_str(&theme.code_inline.render_reset().to_string());
                    self.in_code = false;
                } else {
                    out.push_str(&theme.code_inline.render().to_string());
                    out.push('`');
                    self.in_code = true;
                }
                i += 1;
                continue;
            }

            if self.in_code {
                out.push(chars[i]);
                i += 1;
                continue;
            }

            if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
                if self.in_bold {
                    out.push_str(&bold_style.render_reset().to_string());
                    self.in_bold = false;
                } else {
                    out.push_str(&bold_style.render().to_string());
                    self.in_bold = true;
                }
                i += 2;
                continue;
            }

            if chars[i] == '*' && (i + 1 == len || chars[i + 1] != '*') {
                if self.in_italic {
                    out.push_str(&italic_style.render_reset().to_string());
                    self.in_italic = false;
                } else {
                    out.push_str(&italic_style.render().to_string());
                    self.in_italic = true;
                }
                i += 1;
                continue;
            }

            out.push(chars[i]);
            i += 1;
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
            if !self.emitted_on_current_line {
                let line = std::mem::take(&mut self.current_line);
                out.push_str(&self.process_line(&line, theme));
            } else {
                self.current_line.clear();
                out.push('\n');
            }
        } else if self.emitted_on_current_line {
            out.push('\n');
        }
        self.emitted_on_current_line = false;
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
