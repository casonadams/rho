//! Core `MarkdownRenderer` state machine with spacing normalization.

use super::elements::render_mermaid_block;
use super::highlight::highlight_code_line;
use super::line::{CodeFenceTracker, needs_preceding_blank_line, render_line, should_buffer_line};
use super::stream::InlineStreamTracker;
use super::table::{is_table_line, render_markdown_table};
use crate::ui::theme::Theme;

pub struct MarkdownRenderer {
    code_fence: CodeFenceTracker,
    in_mermaid_block: bool,
    current_line: String,
    emitted_on_current_line: bool,
    table_lines: Vec<String>,
    mermaid_lines: Vec<String>,
    stream_tracker: InlineStreamTracker,
    is_start: bool,
    last_line_was_blank: bool,
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self {
            code_fence: CodeFenceTracker::default(),
            in_mermaid_block: false,
            current_line: String::new(),
            emitted_on_current_line: false,
            table_lines: Vec::new(),
            mermaid_lines: Vec::new(),
            stream_tracker: InlineStreamTracker::default(),
            is_start: true,
            last_line_was_blank: true,
        }
    }
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
                out.push_str(&self.stream_tracker.render_inline_token(chunk, theme));
                out.push('\n');
                self.current_line.clear();
                self.emitted_on_current_line = false;
                self.is_start = false;
                self.last_line_was_blank = false;
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

        if self.emitted_on_current_line {
            return self.stream_tracker.render_inline_token(remaining, theme);
        }
        if self.code_fence.in_code_block
            || self.in_mermaid_block
            || !self.table_lines.is_empty()
            || should_buffer_line(&self.current_line)
        {
            return String::new();
        }

        self.emitted_on_current_line = true;
        self.is_start = false;
        self.last_line_was_blank = false;
        self.stream_tracker.render_inline_token(remaining, theme)
    }

    pub fn flush(&mut self, theme: &Theme) -> String {
        let mut out = String::new();
        self.flush_buffered_blocks(&mut out, theme);
        if !self.current_line.is_empty() {
            if !self.emitted_on_current_line {
                let line = std::mem::take(&mut self.current_line);
                out.push_str(&self.process_line(&line, theme));
            } else {
                self.current_line.clear();
                out.push('\n');
                self.last_line_was_blank = false;
            }
        } else if self.emitted_on_current_line {
            out.push('\n');
            self.last_line_was_blank = false;
        }
        self.emitted_on_current_line = false;
        out
    }

    fn append_block(&mut self, out: &mut String, rendered: &str) {
        if !self.is_start && !self.last_line_was_blank {
            out.push('\n');
        }
        out.push_str(rendered);
        self.is_start = false;
        self.last_line_was_blank = false;
    }

    fn flush_buffered_blocks(&mut self, out: &mut String, theme: &Theme) {
        if !self.table_lines.is_empty() {
            let rendered = render_markdown_table(&std::mem::take(&mut self.table_lines), theme);
            self.append_block(out, &rendered);
        }
        if self.in_mermaid_block && !self.mermaid_lines.is_empty() {
            self.in_mermaid_block = false;
            let rendered = render_mermaid_block(&std::mem::take(&mut self.mermaid_lines).join("\n"), theme);
            self.append_block(out, &rendered);
        }
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
        self.flush_buffered_blocks(&mut out, theme);

        if trimmed.is_empty() {
            if self.code_fence.in_code_block {
                out.push_str(&highlight_code_line(line, self.code_fence.code_lang.as_deref(), theme));
                out.push('\n');
                self.is_start = false;
                self.last_line_was_blank = false;
            } else if !self.is_start && !self.last_line_was_blank {
                out.push('\n');
                self.last_line_was_blank = true;
            }
            return out;
        }

        if needs_preceding_blank_line(trimmed, self.code_fence.in_code_block)
            && !self.is_start
            && !self.last_line_was_blank
        {
            out.push('\n');
        }

        out.push_str(&self.render_line(line, theme));
        out.push('\n');
        self.is_start = false;
        self.last_line_was_blank = false;
        out
    }

    fn check_mermaid_toggle(&mut self, trimmed: &str, theme: &Theme) -> Option<String> {
        if !trimmed.starts_with("```") {
            return None;
        }
        let tag = trimmed.trim_start_matches('`').trim();
        if self.in_mermaid_block {
            self.in_mermaid_block = false;
            let mut out = String::new();
            let src = std::mem::take(&mut self.mermaid_lines).join("\n");
            self.append_block(&mut out, &render_mermaid_block(&src, theme));
            Some(out)
        } else if tag.eq_ignore_ascii_case("mermaid") {
            self.in_mermaid_block = true;
            self.mermaid_lines.clear();
            Some(String::new())
        } else {
            None
        }
    }

    pub fn render_line(&mut self, line: &str, theme: &Theme) -> String {
        render_line(line, &mut self.code_fence, theme)
    }
}
