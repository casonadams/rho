//! Core `TerminalRenderer` struct and its user-facing methods.

use super::formatters::{format_edit_diff, format_session_status, format_thinking_block, format_write_preview};
use super::summary::{approval_heading, bash_approval_details, format_tool_args_summary, to_relative_path};
use super::types::{ApprovalResult, BashApproval, SessionStatus, ToolLine, ToolOutcome, WelcomeDisplay};
use crate::tools::RiskTier;
use crate::ui::block::{BlockFormat, terminal_width};
use crate::ui::markdown::MarkdownRenderer;
use crate::ui::theme::Theme;
use indicatif::{ProgressBar, ProgressStyle};
use std::fmt;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
pub struct TerminalRenderer {
    pub theme: Theme,
    markdown: Arc<Mutex<MarkdownRenderer>>,
}

impl Default for TerminalRenderer {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            markdown: Arc::new(Mutex::new(MarkdownRenderer::new())),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolApprovalChoice {
    ApplyOnce,
    Deny,
}

impl fmt::Display for ToolApprovalChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ApplyOnce => "Apply once",
            Self::Deny => "Deny",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BashApprovalChoice {
    RunOnce,
    Deny,
}

impl fmt::Display for BashApprovalChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RunOnce => "Run once",
            Self::Deny => "Deny",
        })
    }
}

pub(super) fn tool_title_style(is_error: bool) -> anstyle::Style {
    if is_error {
        anstyle::Style::new()
            .bold()
            .fg_color(Some(anstyle::AnsiColor::Red.into()))
    } else {
        anstyle::Style::new().bold()
    }
}

pub(super) fn webfetch_content_kind(arguments: &serde_json::Value) -> &'static str {
    if let Some(format) = arguments.get("format").and_then(serde_json::Value::as_str) {
        return match format.to_ascii_lowercase().as_str() {
            "pdf" => "pdf",
            "json" => "json",
            "csv" => "csv",
            "xml" => "xml",
            _ => "text",
        };
    }
    let url = arguments
        .get("url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if url.ends_with(".pdf") {
        "pdf"
    } else if url.ends_with(".json") {
        "json"
    } else if url.ends_with(".csv") {
        "csv"
    } else if url.ends_with(".xml") || url.ends_with(".rss") || url.ends_with(".atom") {
        "xml"
    } else {
        "text"
    }
}

pub(super) fn format_tool_output_preview(output: &str, fallback: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.is_empty() {
        return fallback.to_string();
    }
    let mut preview = lines.iter().take(8).copied().collect::<Vec<_>>().join("\n");
    if lines.len() > 8 {
        preview.push_str(&format!("\n... ({} more lines)", lines.len() - 8));
    }
    preview
}

fn approval_mode(auto_approve: bool) -> &'static str {
    if auto_approve {
        "auto-approve"
    } else {
        "confirm changes"
    }
}

impl TerminalRenderer {
    pub fn print_welcome(&self, display: &WelcomeDisplay<'_>) {
        let highlight = self.theme.highlight;
        let dim = self.theme.dimmed;
        let session = if display.resumed {
            "resumed session"
        } else {
            "new session"
        };
        let location = std::env::current_dir()
            .ok()
            .map(|path| to_relative_path(&path.display().to_string()))
            .unwrap_or_else(|| ".".to_string());

        println!(
            "\n{highlight}rust-ai{highlight:#} {dim}v{}{dim:#}",
            env!("CARGO_PKG_VERSION")
        );
        println!("{} {dim}via {} | {session}{dim:#}", display.model, display.provider);
        println!("{dim}{location} | {}{dim:#}", approval_mode(display.auto_approve));
        println!("{dim}/help commands | Tab complete | Ctrl+C cancel | Ctrl+D exit{dim:#}\n");
    }

    pub fn print_session_status(&self, display: &SessionStatus<'_>) {
        let dim = self.theme.dimmed;
        let status = format_session_status(display);
        println!("{dim}{status}{dim:#}");
    }

    pub fn start_spinner(&self, message: &str) -> ProgressBar {
        let pb = ProgressBar::new_spinner();
        let style = ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.blue} {msg:.dim}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner());
        pb.set_style(style);
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(80));
        pb
    }

    pub fn start_tool_spinner(&self, name: &str, args: &serde_json::Value) -> ProgressBar {
        let summary = format_tool_args_summary(name, args);
        let msg = format!("{name} {summary}");
        self.start_spinner(&msg)
    }

    pub fn prompt_continue_budget(&self, max_turns: usize) -> bool {
        let header = self.theme.highlight;
        let dim = self.theme.dimmed;
        println!(
            "\n{header}Turn Limit Reached:{header:#} {dim}Agent reached turn budget ({max_turns} calls).{dim:#}\n"
        );
        let approved = inquire::Confirm::new("Continue execution for another 50 turns?")
            .with_default(true)
            .prompt();
        println!();
        approved.unwrap_or(false)
    }

    pub fn prompt_tool_approval(&self, name: &str, args: &serde_json::Value) -> ApprovalResult {
        let header = self.theme.highlight;
        let dim = self.theme.dimmed;
        println!(
            "\n{header}Approve {name}:{header:#} {dim}{}{dim:#}\n",
            format_tool_args_summary(name, args)
        );

        if name == "edit"
            && let Some(diff) = format_edit_diff(args, &self.theme)
        {
            println!("{diff}");
        } else if name == "write"
            && let Some(preview) = format_write_preview(args, &self.theme)
        {
            println!("{preview}");
        }

        let choices = vec![ToolApprovalChoice::ApplyOnce, ToolApprovalChoice::Deny];
        let choice = inquire::Select::new("Action:", choices).prompt();
        println!();
        match choice {
            Ok(ToolApprovalChoice::ApplyOnce) => ApprovalResult::Approved,
            Ok(ToolApprovalChoice::Deny) => self.prompt_denial_feedback(),
            Err(_) => ApprovalResult::Denied { reason: String::new() },
        }
    }

    pub fn prompt_bash_approval(&self, request: BashApproval<'_>) -> ApprovalResult {
        let header = self.theme.highlight;
        let dim = self.theme.dimmed;
        println!("\n{header}{}{header:#}", approval_heading(request.tier));
        for line in bash_approval_details(&request) {
            println!("{dim}{line}{dim:#}");
        }
        println!();

        let actions = vec![BashApprovalChoice::RunOnce, BashApprovalChoice::Deny];
        let starting_cursor = usize::from(request.tier == RiskTier::HighRisk);
        let choice = inquire::Select::new("Action:", actions)
            .with_starting_cursor(starting_cursor)
            .prompt();

        println!();
        match choice {
            Ok(BashApprovalChoice::RunOnce) => ApprovalResult::Approved,
            Ok(BashApprovalChoice::Deny) => self.prompt_denial_feedback(),
            Err(_) => ApprovalResult::Denied { reason: String::new() },
        }
    }

    fn prompt_denial_feedback(&self) -> ApprovalResult {
        let reason = inquire::Text::new("Feedback for the agent (optional):")
            .prompt()
            .unwrap_or_default();
        println!();
        ApprovalResult::Denied {
            reason: reason.trim().to_string(),
        }
    }

    pub fn print_user_block(&self, input: &str) {
        println!();
        print!(
            "{}",
            BlockFormat::new(self.theme.user_message_bg, terminal_width())
                .with_vertical_padding()
                .render_plain(input)
        );
        let _ = io::stdout().flush();
    }

    pub fn finish_tool_line(&self, line: ToolLine<'_>) {
        println!();
        let background = if line.is_error {
            self.theme.tool_error_bg
        } else {
            self.theme.tool_success_bg
        };
        let title = tool_title_style(line.is_error);
        let accent = self.theme.highlight;
        let summary = format_tool_args_summary(line.name, line.arguments);
        let mut content = if line.name == "webfetch" && !line.is_error {
            let url = line
                .arguments
                .get("url")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let status = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
            let dim = self.theme.dimmed;
            let kind = webfetch_content_kind(line.arguments);
            format!(
                "{title}webfetch{title:#}{background}\n{accent}{url}{accent:#}\n{status}fetched ({kind}){status:#}\n{dim}{url}{dim:#}"
            )
        } else {
            format!("{title}{}{title:#}{background} {accent}{summary}{accent:#}", line.name)
        };

        if !line.is_error && line.name == "edit" {
            if let Some(diff) = format_edit_diff(line.arguments, &self.theme) {
                content.push('\n');
                content.push_str(&diff);
            }
        } else if !line.is_error && line.name == "write" {
            if let Some(preview) = format_write_preview(line.arguments, &self.theme) {
                content.push('\n');
                content.push_str(&preview);
            }
        } else if line.name == "bash" || line.is_error {
            content.push_str("\n\n");
            content.push_str(&format_tool_output_preview(line.output, line.output_summary));
        }

        print!(
            "{}",
            BlockFormat::new(background, terminal_width())
                .with_vertical_padding()
                .render_styled(&content)
        );
        let _ = io::stdout().flush();
    }

    pub fn print_token(&self, token: &str) {
        let mut stdout = io::stdout().lock();
        if let Ok(mut markdown) = self.markdown.lock() {
            let rendered = markdown.render_token(token, &self.theme);
            let _ = write!(stdout, "{rendered}");
        } else {
            let _ = write!(stdout, "{token}");
        }
        let _ = stdout.flush();
    }

    pub fn print_thinking_token(&self, token: &str) {
        let dim = self.theme.dimmed;
        let mut stdout = io::stdout().lock();
        let _ = write!(stdout, "{dim}{token}{dim:#}");
        let _ = stdout.flush();
    }

    pub fn flush(&self) {
        let mut stdout = io::stdout().lock();
        if let Ok(mut markdown) = self.markdown.lock() {
            let remaining = markdown.flush(&self.theme);
            if !remaining.is_empty() {
                let _ = write!(stdout, "{remaining}");
            }
        }
        let _ = stdout.flush();
    }

    pub fn print_thinking(&self, thinking_text: &str) {
        let trimmed = thinking_text.trim();
        if trimmed.is_empty() {
            return;
        }
        let formatted = format_thinking_block(trimmed, &self.theme);
        print!("{formatted}");
        let _ = io::stdout().flush();
    }

    pub fn print_tool_start(&self, name: &str, args: &serde_json::Value) {
        let summary = format_tool_args_summary(name, args);
        let header = self.theme.tool_header;
        let dim = self.theme.dimmed;
        println!("{header}{name}{header:#} {dim}{summary}{dim:#}");
    }

    pub fn print_tool_end(&self, outcome: ToolOutcome<'_>) {
        if outcome.is_error {
            let err = self.theme.tool_err;
            println!("{err}{} failed:{err:#} {}", outcome.name, outcome.output_summary);
        } else {
            let ok = self.theme.tool_ok;
            println!("{ok}{}{ok:#}", outcome.name);
        }
    }
}
