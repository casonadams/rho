//! Core `MarkdownRenderer` state machine with spacing normalization.

use super::highlight::CodeHighlighter;
use super::line::{CodeFenceTracker, needs_preceding_blank_line, render_line, should_buffer_line};
use super::mermaid::MermaidBlockTracker;
use super::spacing::SpacingTracker;
use super::stream::InlineStreamTracker;
use super::table::{is_table_line, render_markdown_table};
use crate::ui::theme::Theme;

#[derive(Default)]
pub struct MarkdownRenderer {
    code_fence: CodeFenceTracker,
    code_highlighter: Option<CodeHighlighter<'static>>,
    mermaid: MermaidBlockTracker,
    current_line: String,
    emitted_on_current_line: bool,
    table_lines: Vec<String>,
    stream_tracker: InlineStreamTracker,
    spacing: SpacingTracker,
    width: usize,
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Terminal width available to rendered blocks; `0` leaves content unclipped.
    pub fn set_width(&mut self, width: usize) {
        self.width = width;
        self.mermaid.set_width(width);
    }

    pub fn render_text(&mut self, text: &str, theme: &Theme) -> String {
        let normalized = if text.ends_with('\n') {
            std::borrow::Cow::Borrowed(text)
        } else {
            std::borrow::Cow::Owned(format!("{text}\n"))
        };
        let mut out = self.render_token(&normalized, theme);
        out.push_str(&self.flush(theme));
        out
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
                let mut line = std::mem::take(&mut self.current_line);
                self.process_line(&mut out, &line, theme);
                line.clear();
                self.current_line = line;
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
        if !self.current_line.is_empty() && !self.emitted_on_current_line {
            let mut line = std::mem::take(&mut self.current_line);
            self.process_line(&mut out, &line, theme);
            line.clear();
            self.current_line = line;
        } else if self.emitted_on_current_line {
            out.push_str(&self.stream_tracker.reset_line());
            self.current_line.clear();
            out.push('\n');
            self.spacing.note_content();
        }
        self.flush_buffered_blocks(&mut out, theme);
        self.emitted_on_current_line = false;
        self.code_highlighter = None;
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

    fn try_buffer_block(&mut self, out: &mut String, line: &str, theme: &Theme) -> bool {
        let trimmed = line.trim();
        if let Some(opt_rendered) = self.mermaid.try_render_fence(trimmed, theme) {
            if let Some(rendered) = opt_rendered {
                self.spacing.append_block(out, &rendered);
            }
            return true;
        }
        if self.mermaid.in_block() {
            self.mermaid.push_line(line);
            return true;
        }
        if is_table_line(trimmed) {
            self.table_lines.push(line.to_string());
            return true;
        }
        false
    }

    fn process_empty_line(&mut self, out: &mut String, line: &str, theme: &Theme) {
        if let Some(highlighter) = self.sync_code_highlighter(theme) {
            let highlighted = highlighter.highlight_line(line, theme);
            self.spacing.prepare_content(out);
            out.push_str(&highlighted);
            out.push('\n');
            self.spacing.note_content();
        } else {
            self.spacing.handle_empty_line(out);
        }
    }

    fn process_content_line(&mut self, out: &mut String, line: &str, theme: &Theme) {
        if needs_preceding_blank_line(line.trim(), self.code_fence.in_code_block) {
            self.spacing.ensure_preceding_blank(out);
        }
        self.spacing.prepare_content(out);
        let rendered = self.render_dispatch(line, theme);
        if self.width > 0 && !self.code_fence.in_code_block {
            let wrapped = crate::ui::interactive::wrap_to_width(&rendered, self.width);
            for (idx, wline) in wrapped.iter().enumerate() {
                if idx > 0 {
                    out.push('\n');
                }
                out.push_str(wline);
            }
        } else {
            out.push_str(&rendered);
        }
        out.push('\n');
        self.spacing.note_content();
    }

    fn render_dispatch(&mut self, line: &str, theme: &Theme) -> String {
        if !line.trim().starts_with("```")
            && let Some(highlighter) = self.sync_code_highlighter(theme)
        {
            return highlighter.highlight_line(line, theme);
        }
        let rendered = render_line(line, &mut self.code_fence, theme);
        self.sync_code_highlighter(theme);
        rendered
    }

    /// Drops the cached highlighter when no fence is open and re-resolves it
    /// whenever the fence language changes, so syntect parse state persists
    /// across the lines of one block.
    fn sync_code_highlighter(&mut self, theme: &Theme) -> Option<&mut CodeHighlighter<'static>> {
        if !self.code_fence.in_code_block {
            self.code_highlighter = None;
            return None;
        }
        let lang = self.code_fence.code_lang.as_deref();
        if !self.code_highlighter.as_ref().is_some_and(|h| h.lang() == lang) {
            self.code_highlighter = Some(CodeHighlighter::new(lang, theme));
        }
        self.code_highlighter.as_mut()
    }

    fn process_line(&mut self, out: &mut String, line: &str, theme: &Theme) {
        if self.try_buffer_block(out, line, theme) {
            return;
        }

        self.flush_buffered_blocks(out, theme);

        if line.trim().is_empty() {
            self.process_empty_line(out, line, theme);
        } else {
            self.process_content_line(out, line, theme);
        }
    }

    pub fn render_line(&mut self, line: &str, theme: &Theme) -> String {
        self.render_dispatch(line, theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_prose_wraps_on_word_boundaries() {
        let theme = Theme::default();
        let mut md = MarkdownRenderer::new();
        md.set_width(20);

        let text = "The quick brown fox jumps over the lazy dog";
        let rendered = md.render_text(text, &theme);
        let lines: Vec<&str> = rendered.trim().lines().collect();

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "The quick brown fox");
        assert_eq!(lines[1], "jumps over the lazy");
        assert_eq!(lines[2], "dog");
    }

    #[test]
    fn markdown_code_block_does_not_wrap() {
        let theme = Theme::default();
        let mut md = MarkdownRenderer::new();
        md.set_width(20);

        let code = "```rust\nlet a_very_long_variable_name = \"some long string value\";\n```";
        let rendered = md.render_text(code, &theme);
        let lines: Vec<&str> = rendered.trim().lines().collect();

        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("a_very_long_variable_name"));
        assert!(lines[1].contains("some long string value"));
    }

    #[test]
    fn markdown_unconstrained_when_width_zero() {
        let theme = Theme::default();
        let mut md = MarkdownRenderer::new();

        let text = "The quick brown fox jumps over the lazy dog";
        let rendered = md.render_text(text, &theme);
        let lines: Vec<&str> = rendered.trim().lines().collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "The quick brown fox jumps over the lazy dog");
    }

    #[test]
    fn markdown_styled_inline_preserves_ansi_across_wrap() {
        let theme = Theme::default();
        let mut md = MarkdownRenderer::new();
        md.set_width(12);

        let text = "alpha **beta gamma** delta";
        let rendered = md.render_text(text, &theme);
        let lines: Vec<&str> = rendered.trim().lines().collect();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("alpha"));
        assert!(lines[0].contains("beta"));
        assert!(lines[1].contains("gamma"));
        assert!(lines[1].contains("delta"));
    }

    #[test]
    fn markdown_bullet_list_wraps_on_word_boundaries() {
        let theme = Theme::default();
        let mut md = MarkdownRenderer::new();
        md.set_width(20);

        let text = "- alpha beta gamma delta epsilon";
        let rendered = md.render_text(text, &theme);
        let lines: Vec<&str> = rendered.trim().lines().collect();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("•"));
        assert!(lines[0].contains("alpha beta gamma"));
        assert_eq!(lines[1], "delta epsilon");
    }
}
