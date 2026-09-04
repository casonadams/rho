use super::card::render_headless_tool_card;
use super::formatters::{format_edit_diff, format_thinking_block, format_write_preview};
pub use super::notices::CacheMissNotice;
use crate::ui::interactive::{Activity, InteractiveUi, OutputEvent};
use crate::ui::markdown::MarkdownRenderer;
use crate::ui::render::presenter::InteractiveStreamSink;
use crate::ui::theme::Theme;
use indicatif::{ProgressBar, ProgressStyle};
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::summary::format_tool_args_summary;
use rho_harness_core::presentation::{ToolLine, ToolOutcome};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
pub struct TerminalRenderer {
    pub theme: Theme,
    pub(crate) markdown: Arc<Mutex<MarkdownRenderer>>,
    pub(crate) ui: Option<InteractiveUi>,
    pub(crate) assistant_turn_buffer: Arc<Mutex<String>>,
}

impl Default for TerminalRenderer {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            markdown: Arc::new(Mutex::new(MarkdownRenderer::new())),
            ui: None,
            assistant_turn_buffer: Arc::new(Mutex::new(String::new())),
        }
    }
}

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

    pub fn start_tool_run(&self, name: &str, args: &serde_json::Value) {
        let summary = format_tool_args_summary(name, args);
        let preview = if name == "edit" {
            format_edit_diff(args, &self.theme)
        } else if name == "write" {
            format_write_preview(args, &self.theme, false)
        } else {
            None
        };
        if let Some(ui) = &self.ui {
            let has_running_widget = preview.is_some() || name == "bash";
            if has_running_widget {
                let _ = ui.tool_start(crate::ui::interactive::ToolStartRequest {
                    name: name.to_string(),
                    args_summary: summary,
                    preview,
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

    pub fn start_spinner(&self, message: &str) -> RenderActivity {
        if let Some(ui) = &self.ui {
            let activity = if message.starts_with("thinking") {
                Activity::Thinking
            } else {
                Activity::Working
            };
            let _ = ui.set_activity(activity);
            return RenderActivity::Interactive(ui.clone());
        }
        let pb = ProgressBar::new_spinner();
        let style = ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.cyan} {msg} {elapsed:.dim}")
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

    pub fn finish_tool_line(&self, line: ToolLine) {
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Tool(
                crate::ui::interactive::ToolItem {
                    name: line.name,
                    arguments: line.arguments,
                    is_error: line.is_error,
                    output: line.output,
                    output_summary: line.output_summary,
                    duration_ms: line.duration_ms,
                },
            ));
            return;
        }
        let card = render_headless_tool_card(&line, &self.theme);
        self.write_output(&card);
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
        self.write_output(&rendered);
    }

    pub fn print_thinking_token(&self, token: &str) {
        let dim = self.theme.dimmed;
        self.write_output(&format!("{dim}{token}{dim:#}"));
    }

    pub fn flush(&self) {
        let remaining = self
            .markdown
            .lock()
            .map(|mut markdown| markdown.flush(&self.theme))
            .unwrap_or_default();
        if !remaining.is_empty() {
            self.write_output(&remaining);
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

    pub fn print_thinking(&self, thinking_text: &str) {
        let trimmed = thinking_text.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Thinking(trimmed.to_string()));
        } else {
            let formatted = format_thinking_block(trimmed, &self.theme);
            self.write_output(&formatted);
        }
    }

    pub fn print_tool_start(&self, name: &str, args: &serde_json::Value) {
        let summary = format_tool_args_summary(name, args);
        let header = self.theme.tool_header;
        let dim = self.theme.dimmed;
        self.write_output(&format!("{header}{name}{header:#} {dim}{summary}{dim:#}\n"));
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
