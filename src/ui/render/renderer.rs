use super::formatters::{
    format_bash_approval_card, format_edit_diff, format_session_status, format_thinking_block, format_write_preview,
};
use super::preview::{fetch_content_kind, tool_title_style};
use crate::ui::block::{BlockFormat, terminal_width};
use crate::ui::interactive::{Activity, InteractiveUi, OutputEvent};
use crate::ui::markdown::MarkdownRenderer;
use crate::ui::render::presenter::InteractiveStreamSink;
use crate::ui::theme::Theme;
use indicatif::{ProgressBar, ProgressStyle};
use rho_harness_core::presentation::stream::ToolStreamPort;
use rho_harness_core::presentation::summary::{format_tool_args_summary, read_summary_parts, to_relative_path};
use rho_harness_core::presentation::{SessionStatus, ToolLine, ToolOutcome, WelcomeDisplay};
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
            format_write_preview(args, &self.theme)
        } else {
            None
        };
        if let Some(ui) = &self.ui {
            let _ = ui.tool_start(crate::ui::interactive::ToolStartRequest {
                name: name.to_string(),
                args_summary: summary,
                preview,
            });
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

    pub fn print_welcome(&self, display: &WelcomeDisplay) {
        let location = std::env::current_dir()
            .ok()
            .map(|path| to_relative_path(&path.display().to_string()))
            .unwrap_or_else(|| ".".to_string());
        let item = crate::ui::interactive::WelcomeItem {
            version: env!("CARGO_PKG_VERSION").to_string(),
            model: display.model.to_string(),
            provider: display.provider.to_string(),
            auto_approve: display.auto_approve,
            resumed: display.resumed,
            location,
            tools: display.tools.clone(),
            skills: display.skills.clone(),
            plugins: display.plugins.clone(),
        };
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Welcome(item));
        } else {
            let text = crate::ui::interactive::format_welcome_content(&item, &self.theme);
            self.write_output(&text);
        }
    }

    pub fn print_session_status(&self, display: &SessionStatus) {
        let dim = self.theme.dimmed;
        let status = format_session_status(display);
        let text = format!("{dim}{status}{dim:#}\n");
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Notice(text));
        } else {
            self.write_output(&text);
        }
    }

    pub fn set_extra_status(&self, status: Option<String>) {
        if let Some(ui) = &self.ui {
            let _ = ui.set_extra_status(status);
        }
    }

    pub fn print_block(&self, display: &rho_harness_core::presentation::BlockDisplay) {
        let bg = match display.style.as_str() {
            "error" => self.theme.tool_error_bg,
            "warning" => anstyle::Style::new()
                .bg_color(Some(anstyle::AnsiColor::Yellow.into()))
                .fg_color(Some(anstyle::AnsiColor::Black.into())),
            "success" => self.theme.tool_success_bg,
            _ => self.theme.user_message_bg,
        };
        let formatted_title = if display.title.is_empty() {
            String::new()
        } else {
            let bold = anstyle::Style::new().bold();
            format!("{bold}{}{bold:#}\n\n", display.title)
        };
        let full_text = format!("{formatted_title}{}", display.content);
        let rendered = crate::ui::block::BlockFormat::new(bg, terminal_width())
            .with_vertical_padding()
            .render_styled(&full_text);
        let block_output = format!("\n{rendered}\n");
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Notice(block_output));
        } else {
            self.write_output(&block_output);
        }
    }

    pub fn print_notice(&self, text: &str) {
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Notice(text.to_string()));
        } else {
            self.write_output(text);
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

    pub fn start_bash_run(&self, _command: &str) {}

    pub fn finish_bash_run(&self) {}

    pub fn print_user_block(&self, input: &str) {
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::UserMessage(input.to_string()));
        } else {
            let user = self.theme.prompt;
            self.write_output(&format!("{user}>{user:#} {input}\n\n"));
        }
    }

    pub fn finish_tool_line(&self, line: ToolLine) {
        if let Some(ui) = &self.ui {
            let _ = ui.push_transcript(crate::ui::interactive::TranscriptItem::Tool(
                crate::ui::interactive::ToolItem {
                    name: line.name.clone(),
                    arguments: line.arguments.clone(),
                    is_error: line.is_error,
                    output: line.output.clone(),
                    output_summary: line.output_summary.clone(),
                    duration_ms: line.duration_ms,
                },
            ));
            let _ = ui.tool_end();
            return;
        }
        let background = if line.is_error {
            self.theme.tool_error_bg
        } else {
            self.theme.tool_success_bg
        };
        let title = tool_title_style(line.is_error);
        let accent = self.theme.highlight;
        let summary = format_tool_args_summary(&line.name, &line.arguments);
        let display_name = match line.name.as_str() {
            "web_search" | "websearch" => "search",
            "web_fetch" | "webfetch" => "fetch",
            other => other,
        };
        let mut content = if line.name == "read" && !line.is_error {
            let (path, range) = read_summary_parts(&line.arguments);
            let range_style = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
            format!(
                "{title}read{title:#} {accent}{path}{accent:#}{}",
                range.map_or_else(String::new, |range| format!("{range_style}{range}{range_style:#}"))
            )
        } else if display_name == "fetch" && !line.is_error {
            let url = line
                .arguments
                .get("url")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let status = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
            let kind = fetch_content_kind(&line.arguments);
            format!("{title}{display_name}{title:#} {accent}{url}{accent:#}\n{status}fetched ({kind}){status:#}")
        } else {
            format!("{title}{display_name}{title:#} {accent}{summary}{accent:#}")
        };
        if !line.is_error && line.name == "edit" {
            if let Some(diff) = format_edit_diff(&line.arguments, &self.theme) {
                content.push('\n');
                content.push_str(&diff);
            }
        } else if !line.is_error && line.name == "write" {
            if let Some(preview) = format_write_preview(&line.arguments, &self.theme) {
                content.push('\n');
                content.push_str(&preview);
            }
        } else if line.name == "bash" || line.is_error {
            let raw_output = if !line.output.is_empty() {
                line.output.as_str()
            } else {
                line.output_summary.as_str()
            };
            let clean = raw_output.trim_end();
            if !clean.is_empty() {
                content.push_str("\n\n");
                content.push_str(clean);
            }
        }

        if line.name == "bash"
            && let Some(duration) = line.duration_ms
        {
            let dim = self.theme.dimmed;
            content.push_str("\n\n");
            content.push_str(&format!("{dim}Took {}{dim:#}", super::format_duration_ms(duration)));
        }

        let block = BlockFormat::new(background, terminal_width())
            .with_vertical_padding()
            .render_styled(&content);
        self.write_output(&format!("\n{block}"));
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

    pub fn print_bash_approval_request(&self, request: &rho_harness_core::presentation::BashApproval) {
        let card = format_bash_approval_card(request, &self.theme, terminal_width());
        self.write_output(&format!("\n{card}\n"));
    }
}
