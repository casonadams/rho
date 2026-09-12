//! Central terminal presentation renderer.

mod activity;
mod thinking;
mod tool;

pub use super::notices::CacheMissNotice;
pub use activity::RenderActivity;
pub use thinking::ThinkingStreamTracker;

use crate::ui::interactive::{InteractiveUi, OutputEvent};
use crate::ui::markdown::MarkdownRenderer;
use crate::ui::render::formatters::format_thinking_block;
use crate::ui::render::presenter::InteractiveStreamSink;
use crate::ui::theme::Theme;
use rho_harness_core::presentation::stream::ToolStreamPort;
use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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
        let rendered = self
            .markdown
            .lock()
            .map(|mut markdown| markdown.render_token(token, &self.theme))
            .unwrap_or_else(|_| token.to_string());
        self.stream_output(rendered);
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
                *markdown = MarkdownRenderer::new();
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
}
