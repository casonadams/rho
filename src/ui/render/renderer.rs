//! Central terminal presentation renderer.

use std::io::{self, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

use crate::ui::best_effort;
use crate::ui::interactive::{Activity, InteractiveUi, OutputEvent};
use crate::ui::render::card::render_tool_transcript;
use crate::ui::render::stream::{StreamKind, StreamRenderer};
use crate::ui::theme::Theme;
use rho_engine::tools::file_change::PendingFileChanges;
use rho_harness_core::config::DisplayUiConfig;
use rho_harness_core::presentation::summary::format_tool_args_summary;
use rho_harness_core::presentation::{ToolLine, TurnTelemetry};

pub use super::notices::NoticeKind;

pub enum RenderActivity {
    Progress(ProgressBar),
    Interactive(InteractiveUi),
}

impl RenderActivity {
    pub fn finish_and_clear(self) {
        match self {
            Self::Progress(progress) => progress.finish_and_clear(),
            Self::Interactive(ui) => {
                best_effort(ui.set_activity(Activity::Idle));
            }
        }
    }
}

#[derive(Clone)]
pub struct TerminalRenderer {
    pub theme: Theme,
    pub display: DisplayUiConfig,
    pub(crate) ui: Option<InteractiveUi>,
    pub(crate) assistant_turn_buffer: Arc<Mutex<String>>,
    pub(crate) width: Arc<AtomicUsize>,
    pub(crate) reply_stream: Arc<Mutex<StreamRenderer>>,
    pub(crate) thinking_stream: Arc<Mutex<StreamRenderer>>,
    pub(crate) pending_file_changes: Arc<PendingFileChanges>,
}

impl Default for TerminalRenderer {
    fn default() -> Self {
        let theme = Theme::default();
        Self {
            display: DisplayUiConfig::default(),
            ui: None,
            assistant_turn_buffer: Arc::new(Mutex::new(String::new())),
            width: Arc::new(AtomicUsize::new(0)),
            reply_stream: Arc::new(Mutex::new(StreamRenderer::new(StreamKind::Reply, &theme))),
            thinking_stream: Arc::new(Mutex::new(StreamRenderer::new(StreamKind::Thinking, &theme))),
            pending_file_changes: Arc::new(PendingFileChanges::default()),
            theme,
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

    pub fn has_interactive_ui(&self) -> bool {
        self.ui.is_some()
    }

    pub fn write_output(&self, text: &str) {
        if let Some(ui) = &self.ui {
            best_effort(ui.output(OutputEvent::Text(text.to_string())));
        } else {
            let mut stdout = io::stdout().lock();
            best_effort(stdout.write_all(text.as_bytes()));
            best_effort(stdout.flush());
        }
    }

    pub fn set_display_config(&mut self, display: DisplayUiConfig) {
        self.display = display;
    }

    pub fn print_turn_telemetry(&self, telemetry: &TurnTelemetry) {
        if let Some(line) = super::turn_stats::format_turn_stats_row(telemetry, &self.display, self.width()) {
            self.print_notice(&format!("{line}\n"));
        }
        if let Some(marker) = super::turn_stats::format_cache_miss_marker(telemetry, &self.display, self.width()) {
            self.print_notice(&format!("{marker}\n"));
        }
    }

    pub fn set_width(&self, width: usize) {
        self.width.store(width, Ordering::Relaxed);
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
            best_effort(ui.output(OutputEvent::StreamText(text)));
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
            .reply_stream
            .lock()
            .map(|mut stream| stream.push(token, width, &self.theme))
            .unwrap_or_else(|_| token.to_string());
        self.stream_rendered(rendered);
    }

    pub fn print_thinking_token(&self, token: &str) {
        let width = self.width();
        let rendered = self
            .thinking_stream
            .lock()
            .map(|mut stream| stream.push(token, width, &self.theme))
            .unwrap_or_else(|_| {
                let dim = self.theme.dimmed;
                format!("{dim}{token}{dim:#}")
            });
        self.stream_rendered(rendered);
    }

    fn stream_rendered(&self, rendered: String) {
        if !rendered.is_empty() {
            self.stream_output(rendered);
        }
    }

    fn finish_stream(&self, stream: &Mutex<StreamRenderer>) {
        let remaining = stream
            .lock()
            .map(|mut stream| stream.finish(&self.theme))
            .unwrap_or_default();
        self.stream_rendered(remaining);
    }

    pub fn flush(&self) {
        self.finish_stream(&self.thinking_stream);
        self.finish_stream(&self.reply_stream);
        if let Ok(mut buf) = self.assistant_turn_buffer.lock() {
            let full_text = std::mem::take(&mut *buf);
            if !full_text.is_empty()
                && let Some(ui) = &self.ui
            {
                best_effort(ui.push_transcript(crate::ui::interactive::TranscriptItem::AssistantText(full_text)));
            }
        }
    }

    pub fn finish_thinking(&self, thinking_text: &str) {
        self.finish_stream(&self.thinking_stream);
        let trimmed = thinking_text.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Some(ui) = &self.ui {
            best_effort(ui.push_transcript(crate::ui::interactive::TranscriptItem::Thinking(trimmed.to_string())));
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
            best_effort(ui.set_activity(activity));
            return RenderActivity::Interactive(ui.clone());
        }
        let pb = ProgressBar::new_spinner();
        let style = ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template(" {spinner:.cyan} {msg} {elapsed:.dim}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner());
        pb.set_style(style);
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(64));
        RenderActivity::Progress(pb)
    }

    pub fn start_tool_spinner(&self, name: &str, args: &serde_json::Value) -> RenderActivity {
        let summary = format_tool_args_summary(name, args);
        let msg = format!("{name} {summary}");
        self.start_spinner(&msg)
    }

    pub fn start_tool_run(&self, name: &str, args: &serde_json::Value) {
        let file_change = self.pending_file_changes.note(name, args);
        self.open_tool_card(name, args, file_change);
    }

    /// Opens or updates the card of a call whose arguments are still streaming.
    pub fn preview_tool_run(&self, name: &str, args: &serde_json::Value) {
        let file_change = self.pending_file_changes.note_preview(name, args);
        self.open_tool_card(name, args, file_change);
    }

    fn open_tool_card(
        &self,
        name: &str,
        args: &serde_json::Value,
        file_change: Option<rho_harness_core::presentation::FileChange>,
    ) {
        let summary = if name == "bash" && self.theme.full_bash_command {
            rho_harness_core::presentation::summary::format_bash_full_summary(args)
        } else {
            format_tool_args_summary(name, args)
        };
        if let Some(ui) = &self.ui {
            if crate::ui::interactive::RunningTool::has_live_card(name) {
                best_effort(ui.tool_start(crate::ui::interactive::ToolStartRequest {
                    name: name.to_string(),
                    args_summary: summary,
                    arguments: args.clone(),
                    file_change,
                }));
            } else {
                best_effort(ui.set_running_tool(Some(name.to_string())));
            }
        } else if crate::ui::interactive::RunningTool::has_transcript_card(name) {
            self.print_tool_start(name, args);
        }
    }

    pub fn end_tool_run(&self) {
        self.pending_file_changes.end_preview();
        if let Some(ui) = &self.ui {
            best_effort(ui.tool_end());
        }
    }

    pub fn tool_chunk(&self, chunk: &str) {
        if let Some(ui) = &self.ui {
            best_effort(ui.tool_chunk(chunk.to_string()));
        }
    }

    pub fn finish_tool_line(&self, mut line: ToolLine) {
        if !crate::ui::interactive::RunningTool::has_transcript_card(&line.name) {
            if let Some(ui) = &self.ui {
                best_effort(ui.tool_end());
            }
            return;
        }
        self.pending_file_changes.attach(&mut line);
        if let Some(ui) = &self.ui {
            best_effort(ui.push_transcript(crate::ui::interactive::TranscriptItem::Tool(line)));
            return;
        }
        let card = render_tool_transcript(&line, crate::ui::block::terminal_width(), true, false, &self.theme);
        self.write_output(&card);
    }

    pub fn print_tool_start(&self, name: &str, args: &serde_json::Value) {
        let summary = format_tool_args_summary(name, args);
        let header = self.theme.tool_header;
        let dim = self.theme.dimmed;
        self.write_output(&format!("\n{header}{name}{header:#} {dim}{summary}{dim:#}\n"));
    }
}
