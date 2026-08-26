//! Core `TerminalRenderer` struct and its user-facing methods.

use super::formatters::{format_edit_diff, format_thinking_block, format_write_preview};
use super::summary::{bash_approval_details, format_tool_args_summary, risk_badge};
use super::types::{ApprovalResult, BashApproval, ToolLine, ToolOutcome};
use crate::ui::markdown::MarkdownRenderer;
use crate::ui::theme::Theme;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
pub struct TerminalRenderer {
    pub theme: Theme,
    md: Arc<Mutex<MarkdownRenderer>>,
}

impl Default for TerminalRenderer {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            md: Arc::new(Mutex::new(MarkdownRenderer::new())),
        }
    }
}

impl TerminalRenderer {
    pub fn print_welcome(&self, model: &str, provider: &str) {
        let style = self.theme.highlight;
        let dim = self.theme.dimmed;
        println!("\n{style}rust-ai{style:#} {dim}v0.1.0{dim:#} — minimal agentic coding harness");
        println!("{dim}Model:{dim:#} {model} {dim}({provider}){dim:#}");
        println!("{dim}Type your prompt or /help for slash commands.{dim:#}\n");
    }

    pub fn start_spinner(&self, message: &str) -> ProgressBar {
        let pb = ProgressBar::new_spinner();
        let style = ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.cyan} {msg:.dim}")
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

        let approved = inquire::Confirm::new("Approve execution?").with_default(true).prompt();
        println!();
        match approved {
            Ok(true) => ApprovalResult::Approved,
            Ok(false) => {
                let reason = inquire::Text::new("Reason / feedback for model (optional, Enter to skip):")
                    .prompt()
                    .unwrap_or_default();
                println!();
                ApprovalResult::Denied {
                    reason: reason.trim().to_string(),
                }
            }
            Err(_) => ApprovalResult::Denied { reason: String::new() },
        }
    }

    pub fn prompt_bash_approval(&self, request: BashApproval<'_>) -> ApprovalResult {
        let header = self.theme.highlight;
        let dim = self.theme.dimmed;
        println!(
            "\n{header}[Verify and run command]{header:#} {dim}{}{dim:#}",
            risk_badge(request.tier)
        );
        for line in bash_approval_details(&request) {
            println!("{dim}{line}{dim:#}");
        }
        println!();

        let actions = vec!["Run (Enter)", "Edit command", "Deny / Feedback (Esc)"];
        let choice = inquire::Select::new("Execute:", actions).prompt();

        println!();
        match choice {
            Ok("Run (Enter)") => ApprovalResult::Approved,
            Ok("Edit command") => {
                let edited = inquire::Text::new("$").with_initial_value(request.command).prompt();
                println!();
                match edited {
                    Ok(cmd) if !cmd.trim().is_empty() => ApprovalResult::ApprovedWithCommand(cmd.trim().to_string()),
                    Ok(_) => ApprovalResult::Approved,
                    Err(_) => ApprovalResult::Denied {
                        reason: "Command editing cancelled by user.".to_string(),
                    },
                }
            }
            Ok(_) | Err(_) => {
                let reason = inquire::Text::new("Reason / feedback for model (optional, Enter to skip):")
                    .prompt()
                    .unwrap_or_default();
                println!();
                ApprovalResult::Denied {
                    reason: reason.trim().to_string(),
                }
            }
        }
    }

    pub fn finish_tool_line(&self, line: ToolLine<'_>) {
        let summary = format_tool_args_summary(line.name, line.arguments);
        if line.is_error {
            let err = self.theme.tool_err;
            println!("{err}✖ {}{err:#} {summary} -> {}", line.name, line.output_summary);
        } else {
            let ok = self.theme.tool_ok;
            println!("{ok}✔ {}{ok:#} {summary}", line.name);
            if line.name == "edit"
                && let Some(diff) = format_edit_diff(line.arguments, &self.theme)
            {
                print!("{diff}");
            } else if line.name == "write"
                && let Some(preview) = format_write_preview(line.arguments, &self.theme)
            {
                print!("{preview}");
            }
        }
    }

    pub fn print_token(&self, token: &str) {
        let mut stdout = io::stdout().lock();
        if let Ok(mut md) = self.md.lock() {
            let rendered = md.render_token(token, &self.theme);
            let _ = write!(stdout, "{rendered}");
        } else {
            let _ = write!(stdout, "{token}");
        }
        let _ = stdout.flush();
    }

    pub fn flush(&self) {
        let mut stdout = io::stdout().lock();
        if let Ok(mut md) = self.md.lock() {
            let remaining = md.flush(&self.theme);
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
        println!("{header}⚙{header:#} {name} {dim}{summary}{dim:#}");
    }

    pub fn print_tool_end(&self, outcome: ToolOutcome<'_>) {
        if outcome.is_error {
            let err = self.theme.tool_err;
            println!("{err}✖ {} failed:{err:#} {}", outcome.name, outcome.output_summary);
        } else {
            let ok = self.theme.tool_ok;
            println!("{ok}✔ {}{ok:#}", outcome.name);
        }
    }
}
