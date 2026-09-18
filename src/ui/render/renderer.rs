//! Central terminal presentation renderer.

use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use unicode_width::UnicodeWidthChar;

use crate::ui::interactive::{Activity, InteractiveUi, OutputEvent};
use crate::ui::markdown::MarkdownRenderer;
use crate::ui::render::card::render_headless_tool_card;
use crate::ui::render::formatters::format_thinking_block;
use crate::ui::render::presenter::InteractiveStreamSink;
use crate::ui::theme::Theme;
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::summary::format_tool_args_summary;
use rho_harness_core::presentation::{ToolLine, ToolOutcome};

pub use super::notices::CacheMissNotice;

pub enum RenderActivity {
    Progress(ProgressBar),
    Interactive(InteractiveUi),
}

impl RenderActivity {
    pub fn finish_and_clear(self) {
        match self {
            Self::Progress(progress) => progress.finish_and_clear(),
            Self::Interactive(ui) => {
                let _ = ui.set_activity(Activity::Idle);
            }
        }
    }
}

#[derive(Default)]
pub struct ThinkingStreamTracker {
    col: usize,
    pending_spaces: String,
    pending_spaces_width: usize,
    pending_word: String,
    pending_word_width: usize,
    at_line_start: bool,
}

impl ThinkingStreamTracker {
    pub fn new() -> Self {
        Self {
            col: 0,
            pending_spaces: String::new(),
            pending_spaces_width: 0,
            pending_word: String::new(),
            pending_word_width: 0,
            at_line_start: true,
        }
    }

    pub fn process_token(&mut self, token: &str, width: usize, theme: &Theme) -> String {
        let max_width = if width > 0 { width.saturating_sub(1).max(10) } else { 79 };
        let d = theme.dimmed;
        let mut out = String::new();

        for c in token.chars() {
            if c == '\n' {
                self.commit_pending_word(&mut out, max_width, d);
                out.push('\n');
                self.col = 0;
                self.at_line_start = true;
                self.pending_spaces.clear();
                self.pending_spaces_width = 0;
            } else if c == '\r' {
                continue;
            } else if c == ' ' || c == '\t' {
                if !self.pending_word.is_empty() {
                    self.commit_pending_word(&mut out, max_width, d);
                }
                let cw = UnicodeWidthChar::width(c).unwrap_or(1);
                self.pending_spaces.push(c);
                self.pending_spaces_width += cw;
            } else {
                let cw = UnicodeWidthChar::width(c).unwrap_or(1);
                if self.pending_word_width + cw > max_width {
                    if !self.at_line_start && self.col > 1 {
                        out.push('\n');
                        out.push(' ');
                        self.col = 1;
                        self.pending_spaces.clear();
                        self.pending_spaces_width = 0;
                    } else if self.at_line_start && self.col == 0 {
                        out.push(' ');
                        self.col = 1;
                        self.at_line_start = false;
                    }
                    out.push_str(&format!("{d}{}{d:#}", self.pending_word));
                    out.push('\n');
                    out.push(' ');
                    self.col = 1;
                    self.pending_word.clear();
                    self.pending_word_width = 0;
                }
                self.pending_word.push(c);
                self.pending_word_width += cw;
            }
        }

        out
    }

    fn commit_pending_word(&mut self, out: &mut String, max_width: usize, d: anstyle::Style) {
        if self.pending_word.is_empty() && self.pending_word_width == 0 {
            return;
        }
        let needed = self.pending_spaces_width + self.pending_word_width;
        if !self.at_line_start && self.col + needed > max_width {
            out.push('\n');
            out.push(' ');
            self.col = 1;
            self.pending_spaces.clear();
            self.pending_spaces_width = 0;
        } else if self.at_line_start && self.col == 0 {
            out.push(' ');
            self.col = 1;
            self.at_line_start = false;
        }
        out.push_str(&format!("{d}{}{}{d:#}", self.pending_spaces, self.pending_word));
        self.col += self.pending_spaces_width + self.pending_word_width;
        self.pending_spaces.clear();
        self.pending_spaces_width = 0;
        self.pending_word.clear();
        self.pending_word_width = 0;
    }

    pub fn flush(&mut self, theme: &Theme) -> String {
        let mut out = String::new();
        let d = theme.dimmed;
        if !self.pending_word.is_empty() {
            if self.at_line_start && self.col == 0 {
                out.push(' ');
            }
            out.push_str(&format!("{d}{}{}{d:#}", self.pending_spaces, self.pending_word));
        }
        self.col = 0;
        self.pending_spaces.clear();
        self.pending_spaces_width = 0;
        self.pending_word.clear();
        self.pending_word_width = 0;
        self.at_line_start = true;
        out
    }
}

#[derive(Clone)]
pub struct TerminalRenderer {
    pub theme: Theme,
    pub(crate) markdown: Arc<Mutex<MarkdownRenderer>>,
    pub(crate) ui: Option<InteractiveUi>,
    pub(crate) assistant_turn_buffer: Arc<Mutex<String>>,
    pub(crate) width: Arc<AtomicUsize>,
    pub(crate) thinking_stream: Arc<Mutex<ThinkingStreamTracker>>,
}

impl Default for TerminalRenderer {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            markdown: Arc::new(Mutex::new(MarkdownRenderer::new())),
            ui: None,
            assistant_turn_buffer: Arc::new(Mutex::new(String::new())),
            width: Arc::new(AtomicUsize::new(0)),
            thinking_stream: Arc::new(Mutex::new(ThinkingStreamTracker::new())),
        }
    }
}

impl TerminalRenderer {
    pub fn with_ui(ui: InteractiveUi) -> Self {
        Self {
            ui: Some(ui),
            ..Self::default()
        }
    }

    pub fn stream_port(&self) -> ToolStreamPort {
        ToolStreamPort::new(
            self.ui
                .clone()
                .map(|ui| std::sync::Arc::new(InteractiveStreamSink(Some(ui))) as _),
        )
    }

    pub fn has_interactive_ui(&self) -> bool {
        self.ui.is_some()
    }

    pub fn write_output(&self, text: &str) {
        if let Some(ui) = &self.ui {
            let _ = ui.output(OutputEvent::Text(text.to_string()));
        } else {
            let mut stdout = io::stdout().lock();
            let _ = stdout.write_all(text.as_bytes());
            let _ = stdout.flush();
        }
    }

    pub fn set_extra_status(&self, status: Option<String>) {
        if let Some(ui) = &self.ui {
            let _ = ui.set_extra_status(status);
        }
    }

    pub fn set_width(&self, width: usize) {
        self.width.store(width, Ordering::Relaxed);
        if let Ok(mut md) = self.markdown.lock() {
            md.set_width(width);
        }
    }

    pub fn width(&self) -> usize {
        let w = self.width.load(Ordering::Relaxed);
        if w > 0 {
            w
        } else {
            crossterm::terminal::size().map(|(w, _)| w as usize).unwrap_or(80)
        }
    }

    pub fn stream_output(&self, text: String) {
        if let Some(ui) = &self.ui {
            let _ = ui.output(OutputEvent::StreamText(text));
        } else {
            self.write_output(&text);
        }
    }

    pub fn print_token(&self, token: &str) {
        if let Ok(mut buf) = self.assistant_turn_buffer.lock() {
            buf.push_str(token);
        }
        let width = self.width.load(Ordering::Relaxed);
        let rendered = self
            .markdown
            .lock()
            .map(|mut markdown| {
                if markdown.width() == 0 && width > 0 {
                    markdown.set_width(width);
                }
                markdown.render_token(token, &self.theme)
            })
            .unwrap_or_else(|_| token.to_string());
        if !rendered.is_empty() {
            self.stream_output(rendered);
        }
    }

    pub fn print_thinking_token(&self, token: &str) {
        let width = self.width();
        let rendered = self
            .thinking_stream
            .lock()
            .map(|mut tracker| tracker.process_token(token, width, &self.theme))
            .unwrap_or_else(|_| {
                let dim = self.theme.dimmed;
                format!("{dim}{token}{dim:#}")
            });
        if !rendered.is_empty() {
            self.stream_output(rendered);
        }
    }

    pub fn flush(&self) {
        if let Ok(mut tracker) = self.thinking_stream.lock() {
            let remaining = tracker.flush(&self.theme);
            if !remaining.is_empty() {
                self.stream_output(remaining);
            }
        }
        let remaining = self
            .markdown
            .lock()
            .map(|mut markdown| {
                let out = markdown.flush(&self.theme);
                let w = markdown.width();
                let mut new_md = MarkdownRenderer::new();
                if w > 0 {
                    new_md.set_width(w);
                }
                *markdown = new_md;
                out
            })
            .unwrap_or_default();
        if !remaining.is_empty() {
            self.stream_output(remaining);
        }
        if let Ok(mut buf) = self.assistant_turn_buffer.lock() {
            let full_text = std::mem::take(&mut *buf);
            if !full_text.is_empty()
                && let Some(ui) = &self.ui
            {
                let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::AssistantText(full_text));
            }
        }
    }

    pub fn finish_thinking(&self, thinking_text: &str) {
        if let Ok(mut tracker) = self.thinking_stream.lock() {
            let remaining = tracker.flush(&self.theme);
            if !remaining.is_empty() {
                self.stream_output(remaining);
            }
        }
        let trimmed = thinking_text.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Thinking(trimmed.to_string()));
        }
    }

    pub fn print_thinking(&self, thinking_text: &str) {
        let trimmed = thinking_text.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Thinking(trimmed.to_string()));
        } else {
            let formatted = format_thinking_block(trimmed, &self.theme, self.width());
            self.write_output(&formatted);
        }
    }

    pub fn start_spinner(&self, message: &str) -> RenderActivity {
        if let Some(ui) = &self.ui {
            let activity = if message.starts_with("thinking") {
                Activity::Thinking
            } else if message.starts_with("compacting") {
                Activity::Compacting
            } else {
                Activity::Working
            };
            let _ = ui.set_activity(activity);
            return RenderActivity::Interactive(ui.clone());
        }
        let pb = ProgressBar::new_spinner();
        let style = ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template(" {spinner:.cyan} {msg} {elapsed:.dim}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner());
        pb.set_style(style);
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(80));
        RenderActivity::Progress(pb)
    }

    pub fn start_tool_spinner(&self, name: &str, args: &serde_json::Value) -> RenderActivity {
        let summary = format_tool_args_summary(name, args);
        let msg = format!("{name} {summary}");
        self.start_spinner(&msg)
    }

    pub fn start_tool_run(&self, name: &str, args: &serde_json::Value) {
        let summary = format_tool_args_summary(name, args);
        if let Some(ui) = &self.ui {
            if name == "bash" {
                let _ = ui.tool_start(crate::ui::interactive::ToolStartRequest {
                    name: name.to_string(),
                    args_summary: summary,
                    preview: None,
                });
            } else {
                let _ = ui.set_running_tool(Some(name.to_string()));
            }
        } else {
            self.print_tool_start(name, args);
        }
    }

    pub fn tool_chunk(&self, chunk: &str) {
        if let Some(ui) = &self.ui {
            let _ = ui.tool_chunk(chunk.to_string());
        }
    }

    pub fn finish_tool_line(&self, line: ToolLine) {
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Tool(line));
            return;
        }
        let card = render_headless_tool_card(&line, &self.theme);
        self.write_output(&card);
    }

    pub fn print_tool_start(&self, name: &str, args: &serde_json::Value) {
        let summary = format_tool_args_summary(name, args);
        let header = self.theme.tool_header;
        let dim = self.theme.dimmed;
        self.write_output(&format!("\n{header}{name}{header:#} {dim}{summary}{dim:#}\n"));
    }

    pub fn print_tool_end(&self, outcome: ToolOutcome) {
        if outcome.is_error {
            let err = self.theme.tool_err;
            self.write_output(&format!(
                "{err}{} failed:{err:#} {}\n",
                outcome.name, outcome.output_summary
            ));
        } else {
            let ok = self.theme.tool_ok;
            self.write_output(&format!("{ok}{}{ok:#}\n", outcome.name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    fn strip_ansi(s: &str) -> String {
        crate::ui::block::ANSI_PATTERN.replace_all(s, "").to_string()
    }

    #[test]
    fn stream_thinking_wraps_on_word_boundaries() {
        let mut tracker = ThinkingStreamTracker::new();
        let theme = Theme::default();
        let mut out = String::new();
        out.push_str(&tracker.process_token("The quick brown fox jumps over ", 20, &theme));
        out.push_str(&tracker.flush(&theme));
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines.len() >= 2);
        assert!(!out.contains("qui-\nck"));
    }

    #[test]
    fn stream_thinking_preserves_single_space_indent_on_wrapped_lines() {
        let mut tracker = ThinkingStreamTracker::new();
        let theme = Theme::default();
        let mut out = String::new();
        out.push_str(&tracker.process_token("alpha beta gamma delta epsilon zeta", 15, &theme));
        out.push_str(&tracker.flush(&theme));
        let lines: Vec<&str> = out.lines().collect();
        for line in &lines {
            let stripped = strip_ansi(line);
            assert!(stripped.starts_with(' '));
        }
    }

    #[test]
    fn stream_thinking_preserves_natural_line_breaks() {
        let mut tracker = ThinkingStreamTracker::new();
        let theme = Theme::default();
        let mut out = String::new();
        out.push_str(&tracker.process_token("line one\nline two", 80, &theme));
        out.push_str(&tracker.flush(&theme));
        let stripped = strip_ansi(&out);
        assert!(stripped.contains("line one\n"));
        assert!(stripped.contains("line two"));
    }

    #[test]
    fn stream_thinking_preserves_token_split_natural_line_breaks() {
        let mut tracker = ThinkingStreamTracker::new();
        let theme = Theme::default();
        let mut out = String::new();
        out.push_str(&tracker.process_token("step 1", 80, &theme));
        out.push_str(&tracker.process_token("\n", 80, &theme));
        out.push_str(&tracker.process_token("step 2", 80, &theme));
        out.push_str(&tracker.flush(&theme));
        let stripped = strip_ansi(&out);
        assert!(stripped.contains("step 1\n"));
        assert!(stripped.contains("step 2"));
    }

    #[test]
    fn stream_thinking_preserves_natural_paragraph_breaks() {
        let mut tracker = ThinkingStreamTracker::new();
        let theme = Theme::default();
        let mut out = String::new();
        out.push_str(&tracker.process_token("para one\n\npara two", 80, &theme));
        out.push_str(&tracker.flush(&theme));
        let stripped = strip_ansi(&out);
        assert!(stripped.contains("para one\n\n"));
        assert!(stripped.contains("para two"));
    }

    #[test]
    fn stream_thinking_handles_crlf_line_breaks() {
        let mut tracker = ThinkingStreamTracker::new();
        let theme = Theme::default();
        let mut out = String::new();
        out.push_str(&tracker.process_token("line one\r\nline two", 80, &theme));
        out.push_str(&tracker.flush(&theme));
        let stripped = strip_ansi(&out);
        assert!(!stripped.contains('\r'));
        assert!(stripped.contains("line one\n"));
        assert!(stripped.contains("line two"));
    }
}
