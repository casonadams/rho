//! Central terminal presentation renderer.

use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

use crate::ui::interactive::{Activity, InteractiveUi, OutputEvent};
use crate::ui::markdown::MarkdownRenderer;
use crate::ui::render::card::render_headless_tool_card;
use crate::ui::render::formatters::format_thinking_block;
use crate::ui::render::presenter::InteractiveStreamSink;
use crate::ui::render::thinking_stream::ThinkingStreamTracker;
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
