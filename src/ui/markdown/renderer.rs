//! Core `MarkdownRenderer` state machine with spacing normalization.

use super::highlight::highlight_code_line;
use super::line::{CodeFenceTracker, needs_preceding_blank_line, render_line, should_buffer_line};
use super::mermaid::MermaidBlockTracker;
use super::spacing::SpacingTracker;
use super::stream::InlineStreamTracker;
use super::table::{is_table_line, render_markdown_table};
use crate::ui::theme::Theme;

#[derive(Default)]
pub struct MarkdownRenderer {
    code_fence: CodeFenceTracker,
    mermaid: MermaidBlockTracker,
    current_line: String,
    emitted_on_current_line: bool,
    table_lines: Vec<String>,
    stream_tracker: InlineStreamTracker,
    spacing: SpacingTracker,
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
                out.push_str(&self.stream_tracker.reset_line());
                out.push('\n');
                self.current_line.clear();
                self.emitted_on_current_line = false;
                self.spacing.note_content();
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
            || self.mermaid.in_block()
            || !self.table_lines.is_empty()
            || should_buffer_line(&self.current_line)
        {
            return String::new();
        }
        self.emitted_on_current_line = true;
        let mut out = String::new();
        self.spacing.prepare_content(&mut out);
        out.push_str(&self.stream_tracker.render_inline_token(&self.current_line, theme));
        out
    }

    pub fn flush(&mut self, theme: &Theme) -> String {
        let mut out = String::new();
        self.flush_buffered_blocks(&mut out, theme);
        if !self.current_line.is_empty() && !self.emitted_on_current_line {
            let line = std::mem::take(&mut self.current_line);
            out.push_str(&self.process_line(&line, theme));
        } else if self.emitted_on_current_line {
            out.push_str(&self.stream_tracker.reset_line());
            self.current_line.clear();
            out.push('\n');
            self.spacing.note_content();
        }
        self.emitted_on_current_line = false;
        out
    }

    fn flush_buffered_blocks(&mut self, out: &mut String, theme: &Theme) {
        if !self.table_lines.is_empty() {
            let rendered = render_markdown_table(&std::mem::take(&mut self.table_lines), theme);
            self.spacing.append_block(out, &rendered);
        }
        if let Some(rendered) = self.mermaid.flush_rendered(theme) {
            self.spacing.append_block(out, &rendered);
        }
    }

    fn process_line(&mut self, line: &str, theme: &Theme) -> String {
        let trimmed = line.trim();

        if let Some(opt_rendered) = self.mermaid.try_render_fence(trimmed, theme) {
            let mut out = String::new();
            if let Some(rendered) = opt_rendered {
                self.spacing.append_block(&mut out, &rendered);
            }
            return out;
        }
        if self.mermaid.in_block() {
            self.mermaid.push_line(line);
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
                self.spacing.prepare_content(&mut out);
                out.push_str(&highlight_code_line(line, self.code_fence.code_lang.as_deref(), theme));
                out.push('\n');
                self.spacing.note_content();
            } else {
                self.spacing.handle_empty_line(&mut out);
            }
            return out;
        }

        if needs_preceding_blank_line(trimmed, self.code_fence.in_code_block) {
            self.spacing.ensure_preceding_blank(&mut out);
        }

        self.spacing.prepare_content(&mut out);
        out.push_str(&render_line(line, &mut self.code_fence, theme));
        out.push('\n');
        self.spacing.note_content();
        out
    }

    pub fn render_line(&mut self, line: &str, theme: &Theme) -> String {
        render_line(line, &mut self.code_fence, theme)
    }
}
