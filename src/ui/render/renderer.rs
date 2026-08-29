//! Core `TerminalRenderer` struct and its user-facing methods.

use super::formatters::{
    format_bash_approval_card, format_edit_diff, format_session_status, format_thinking_block, format_write_preview,
};
use super::summary::{clean_command_paths, format_tool_args_summary, read_summary_parts, to_relative_path};
use super::types::{ApprovalResult, BashApproval, SessionStatus, ToolLine, ToolOutcome, WelcomeDisplay};
use crate::tools::{QuestionPort, RiskTier};
use crate::ui::block::{BlockFormat, terminal_width};
use crate::ui::interactive::{
    Activity, InteractionOption, InteractionPrompt, InteractionResponse, InteractiveUi, OutputEvent,
};
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
    ui: Option<InteractiveUi>,
}

impl Default for TerminalRenderer {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            markdown: Arc::new(Mutex::new(MarkdownRenderer::new())),
            ui: None,
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum BashApprovalChoice {
    AllowOnce,
    AllowForSession(String),
    Deny,
}

impl fmt::Display for BashApprovalChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllowOnce => formatter.write_str("Allow once"),
            Self::AllowForSession(scope) => write!(formatter, "Allow {scope} for session"),
            Self::Deny => formatter.write_str("Deny"),
        }
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

fn interaction_option(label: &str) -> InteractionOption {
    InteractionOption {
        label: label.to_string(),
        description: None,
    }
}

fn denied(reason: String) -> ApprovalResult {
    ApprovalResult::Denied {
        reason: reason.trim().to_string(),
    }
}

impl TerminalRenderer {
    pub fn with_ui(ui: InteractiveUi) -> Self {
        Self {
            ui: Some(ui),
            ..Self::default()
        }
    }

    pub fn question_port(&self) -> QuestionPort {
        crate::ui::question::question_port(self.ui.clone())
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

        self.write_output(&format!(
            "\n{highlight}rho{highlight:#} {dim}v{}{dim:#}\n{} {dim}via {} | {session}{dim:#}\n{dim}{location} | {}{dim:#}\n{dim}/help commands | Tab complete | Ctrl+C cancel | Ctrl+D exit{dim:#}\n\n",
            env!("CARGO_PKG_VERSION"),
            display.model,
            display.provider,
            approval_mode(display.auto_approve)
        ));
    }

    pub fn print_session_status(&self, display: &SessionStatus<'_>) {
        let dim = self.theme.dimmed;
        let status = format_session_status(display);
        self.write_output(&format!("{dim}{status}{dim:#}\n"));
    }

    pub fn start_spinner(&self, message: &str) -> RenderActivity {
        if let Some(ui) = &self.ui {
            let activity = if message.starts_with("thinking") {
                Activity::Thinking
            } else {
                Activity::Tool(message.to_string())
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

    pub async fn prompt_continue_budget(&self, max_turns: usize) -> bool {
        if let Some(ui) = &self.ui {
            let response = ui
                .request(InteractionPrompt {
                    title: "Turn Limit Reached".to_string(),
                    body: format!("Agent reached turn budget ({max_turns} calls)."),
                    options: vec![
                        interaction_option("Continue for another 50 turns"),
                        interaction_option("Stop"),
                    ],
                    initial_selection: 0,
                    allow_custom: false,
                })
                .await;
            return matches!(response, Ok(InteractionResponse::Selected(0)));
        }

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

    pub async fn prompt_tool_approval(&self, name: &str, args: &serde_json::Value) -> ApprovalResult {
        if let Some(ui) = &self.ui {
            let summary = format_tool_args_summary(name, args);
            let mut body = format!("tool   {name}\nscope  {summary}");
            if name == "edit"
                && let Some(diff) = format_edit_diff(args, &self.theme)
            {
                body.push_str("\n\n");
                body.push_str(&diff);
            } else if name == "write"
                && let Some(preview) = format_write_preview(args, &self.theme)
            {
                body.push_str("\n\n");
                body.push_str(&preview);
            }
            let response = ui
                .request(InteractionPrompt {
                    title: format!("Approve {name}"),
                    body,
                    options: vec![
                        InteractionOption {
                            label: "Allow".to_string(),
                            description: Some("Allow this single invocation".to_string()),
                        },
                        InteractionOption {
                            label: "Deny with reason".to_string(),
                            description: Some("Deny and provide feedback to the agent".to_string()),
                        },
                    ],
                    initial_selection: 0,
                    allow_custom: false,
                })
                .await;
            return match response {
                Ok(InteractionResponse::Selected(0)) => ApprovalResult::Approved,
                Ok(InteractionResponse::Selected(1)) => self.prompt_denial_feedback().await,
                Ok(InteractionResponse::Custom(reason)) => denied(reason),
                Ok(InteractionResponse::Selected(_) | InteractionResponse::Cancelled) | Err(_) => denied(String::new()),
            };
        }

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
        let choice =
            inquire::Select::new("Action:", vec![ToolApprovalChoice::ApplyOnce, ToolApprovalChoice::Deny]).prompt();
        println!();
        match choice {
            Ok(ToolApprovalChoice::ApplyOnce) => ApprovalResult::Approved,
            Ok(ToolApprovalChoice::Deny) => self.prompt_denial_feedback().await,
            Err(_) => denied(String::new()),
        }
    }

    pub async fn prompt_bash_approval(&self, request: BashApproval<'_>) -> ApprovalResult {
        let mut actions = vec![BashApprovalChoice::AllowOnce];
        if let Some(patterns) = crate::tools::analyze_command_safety(request.command).session_patterns {
            actions.push(BashApprovalChoice::AllowForSession(patterns.join("; ")));
        }
        actions.push(BashApprovalChoice::Deny);

        let starting_cursor = if request.tier == RiskTier::HighRisk {
            actions.len() - 1
        } else {
            0
        };

        if let Some(ui) = &self.ui {
            let mut options = vec![InteractionOption {
                label: "Allow".to_string(),
                description: Some("Allow this single invocation".to_string()),
            }];
            if let Some(patterns) = crate::tools::analyze_command_safety(request.command).session_patterns {
                options.push(InteractionOption {
                    label: "Allow for session".to_string(),
                    description: Some(format!("Allow {} for session", patterns.join("; "))),
                });
            }
            options.push(InteractionOption {
                label: "Deny with reason".to_string(),
                description: Some("Deny and provide feedback to the agent".to_string()),
            });

            let mut body = format!("tool   bash\nscope  {}", clean_command_paths(request.command));
            if request.tier == RiskTier::HighRisk && !request.reasons.is_empty() {
                body.push_str("\n\n");
                body.push_str(&request.reasons.join("\n"));
            }
            let response = ui
                .request(InteractionPrompt {
                    title: "Bash command requires approval".to_string(),
                    body,
                    options: options.clone(),
                    initial_selection: starting_cursor,
                    allow_custom: false,
                })
                .await;
            return match response {
                Ok(InteractionResponse::Selected(index)) => match options.get(index).map(|opt| opt.label.as_str()) {
                    Some("Allow") => ApprovalResult::Approved,
                    Some("Allow for session") => ApprovalResult::ApprovedForSession,
                    Some("Deny with reason") => self.prompt_denial_feedback().await,
                    _ => denied(String::new()),
                },
                Ok(InteractionResponse::Custom(reason)) => denied(reason),
                Ok(InteractionResponse::Cancelled) | Err(_) => denied(String::new()),
            };
        }

        println!();
        print!("{}", format_bash_approval_card(&request, &self.theme, terminal_width()));
        println!();
        let choice = inquire::Select::new("Permission:", actions)
            .with_starting_cursor(starting_cursor)
            .prompt();
        println!();
        match choice {
            Ok(BashApprovalChoice::AllowOnce) => ApprovalResult::Approved,
            Ok(BashApprovalChoice::AllowForSession(_)) => ApprovalResult::ApprovedForSession,
            Ok(BashApprovalChoice::Deny) => self.prompt_denial_feedback().await,
            Err(_) => denied(String::new()),
        }
    }

    async fn prompt_denial_feedback(&self) -> ApprovalResult {
        if let Some(ui) = &self.ui {
            let response = ui
                .request(InteractionPrompt {
                    title: "Deny operation".to_string(),
                    body: "Optionally provide feedback for the agent.".to_string(),
                    options: vec![interaction_option("Deny without feedback")],
                    initial_selection: 0,
                    allow_custom: true,
                })
                .await;
            return match response {
                Ok(InteractionResponse::Custom(reason)) => denied(reason),
                Ok(InteractionResponse::Selected(_) | InteractionResponse::Cancelled) | Err(_) => denied(String::new()),
            };
        }

        let reason = inquire::Text::new("Feedback for the agent (optional):")
            .prompt()
            .unwrap_or_default();
        println!();
        denied(reason)
    }

    pub fn print_user_block(&self, input: &str) {
        let block = BlockFormat::new(self.theme.user_message_bg, terminal_width())
            .with_vertical_padding()
            .render_plain(input);
        self.write_output(&format!("\n{block}"));
    }

    pub fn finish_tool_line(&self, line: ToolLine<'_>) {
        let background = if line.is_error {
            self.theme.tool_error_bg
        } else {
            self.theme.tool_success_bg
        };
        let title = tool_title_style(line.is_error);
        let accent = self.theme.highlight;
        let summary = format_tool_args_summary(line.name, line.arguments);
        let mut content = if line.name == "read" && !line.is_error {
            let (path, range) = read_summary_parts(line.arguments);
            let range_style = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
            format!(
                "{title}read{title:#} {accent}{path}{accent:#}{}",
                range.map_or_else(String::new, |range| format!("{range_style}{range}{range_style:#}"))
            )
        } else if line.name == "webfetch" && !line.is_error {
            let url = line
                .arguments
                .get("url")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let status = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
            let dim = self.theme.dimmed;
            let kind = webfetch_content_kind(line.arguments);
            format!(
                "{title}webfetch{title:#}\n{accent}{url}{accent:#}\n{status}fetched ({kind}){status:#}\n{dim}{url}{dim:#}"
            )
        } else {
            format!("{title}{}{title:#} {accent}{summary}{accent:#}", line.name)
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

        let block = BlockFormat::new(background, terminal_width())
            .with_vertical_padding()
            .render_styled(&content);
        self.write_output(&format!("\n{block}"));
    }

    pub fn print_token(&self, token: &str) {
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
    }

    pub fn print_thinking(&self, thinking_text: &str) {
        let trimmed = thinking_text.trim();
        if trimmed.is_empty() {
            return;
        }
        let formatted = format_thinking_block(trimmed, &self.theme);
        self.write_output(&formatted);
    }

    pub fn print_tool_start(&self, name: &str, args: &serde_json::Value) {
        let summary = format_tool_args_summary(name, args);
        let header = self.theme.tool_header;
        let dim = self.theme.dimmed;
        self.write_output(&format!("{header}{name}{header:#} {dim}{summary}{dim:#}\n"));
    }

    pub fn print_tool_end(&self, outcome: ToolOutcome<'_>) {
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
