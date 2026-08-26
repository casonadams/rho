use crate::tools::bash_ast::RiskTier;
use crate::ui::markdown::MarkdownRenderer;
use crate::ui::theme::Theme;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, Write};
use std::path::Path;
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

pub enum ApprovalResult {
    Approved,
    Denied { reason: String },
}

pub struct BashApproval<'a> {
    pub command: &'a str,
    pub tier: RiskTier,
    pub reasons: &'a [String],
}

pub struct ToolLine<'a> {
    pub name: &'a str,
    pub arguments: &'a serde_json::Value,
    pub is_error: bool,
    pub output_summary: &'a str,
}

pub struct ToolOutcome<'a> {
    pub name: &'a str,
    pub is_error: bool,
    pub output_summary: &'a str,
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
            "\n  {header}Turn Limit Reached:{header:#} {dim}Agent reached turn budget ({max_turns} calls).{dim:#}\n"
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
            "\n  {header}Approve {name}:{header:#} {dim}{}{dim:#}\n",
            format_tool_args_summary(name, args)
        );

        if name == "edit"
            && let Some(diff) = format_edit_diff(args, &self.theme)
        {
            println!("{diff}");
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
                let reason = if reason.trim().is_empty() {
                    "Execution denied by user.".to_string()
                } else {
                    format!("Denied by user: {}", reason.trim())
                };
                ApprovalResult::Denied { reason }
            }
            Err(_) => ApprovalResult::Denied {
                reason: "Execution canceled by user.".to_string(),
            },
        }
    }

    pub fn prompt_bash_approval(&self, request: BashApproval<'_>) -> ApprovalResult {
        let header = self.theme.highlight;
        let dim = self.theme.dimmed;
        println!(
            "\n  {header}{}{header:#} Execute Shell Command:",
            risk_badge(request.tier)
        );
        for line in bash_approval_details(&request) {
            println!("  {dim}{line}{dim:#}");
        }
        println!();

        let approved = inquire::Confirm::new("Approve execution?").with_default(true).prompt();
        println!();
        match approved {
            Ok(true) => ApprovalResult::Approved,
            Ok(false) => {
                let reason = inquire::Text::new("Reason / feedback for model (optional, Enter to skip):")
                    .prompt()
                    .unwrap_or_default();
                let reason = if reason.trim().is_empty() {
                    "Execution denied by user.".to_string()
                } else {
                    format!("Denied by user: {}", reason.trim())
                };
                ApprovalResult::Denied { reason }
            }
            Err(_) => ApprovalResult::Denied {
                reason: "Execution canceled by user.".to_string(),
            },
        }
    }

    pub fn finish_tool_line(&self, line: ToolLine<'_>) {
        let summary = format_tool_args_summary(line.name, line.arguments);
        if line.is_error {
            let err = self.theme.tool_err;
            println!("  {err}✖ {}{err:#} {summary} -> {}", line.name, line.output_summary);
        } else {
            let ok = self.theme.tool_ok;
            println!("  {ok}✔ {}{ok:#} {summary}", line.name);
            if line.name == "edit"
                && let Some(diff) = format_edit_diff(line.arguments, &self.theme)
            {
                print!("{diff}");
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
        println!("  {header}⚙{header:#} {name} {dim}{summary}{dim:#}");
    }

    pub fn print_tool_end(&self, outcome: ToolOutcome<'_>) {
        if outcome.is_error {
            let err = self.theme.tool_err;
            println!("  {err}✖ {} failed:{err:#} {}", outcome.name, outcome.output_summary);
        } else {
            let ok = self.theme.tool_ok;
            println!("  {ok}✔ {}{ok:#}", outcome.name);
        }
    }
}

struct DiffFormatter<'a> {
    theme: &'a Theme,
    out: String,
}

impl<'a> DiffFormatter<'a> {
    fn new(theme: &'a Theme) -> Self {
        Self {
            theme,
            out: String::new(),
        }
    }

    fn append_removals(&mut self, text: &str) {
        let red = self.theme.tool_err;
        for line in text.lines().take(8) {
            self.out.push_str(&format!("  {red}- {line}{red:#}\n"));
        }
        let count = text.lines().count();
        if count > 8 {
            let dim = self.theme.dimmed;
            self.out
                .push_str(&format!("  {dim}  ... ({} more lines){dim:#}\n", count - 8));
        }
    }

    fn append_additions(&mut self, text: &str) {
        let green = self.theme.tool_ok;
        for line in text.lines().take(8) {
            self.out.push_str(&format!("  {green}+ {line}{green:#}\n"));
        }
        let count = text.lines().count();
        if count > 8 {
            let dim = self.theme.dimmed;
            self.out
                .push_str(&format!("  {dim}  ... ({} more lines){dim:#}\n", count - 8));
        }
    }

    fn format_entry(&mut self, idx: usize, edit: &serde_json::Value) {
        if idx > 0 {
            let dim = self.theme.dimmed;
            self.out.push_str(&format!("  {dim}── edit #{} ──{dim:#}\n", idx + 1));
        }
        let old_text = edit.get("oldText").and_then(|v| v.as_str()).unwrap_or("");
        let new_text = edit.get("newText").and_then(|v| v.as_str()).unwrap_or("");
        self.append_removals(old_text);
        self.append_additions(new_text);
    }
}

pub fn format_edit_diff(args: &serde_json::Value, theme: &Theme) -> Option<String> {
    let edits = args.get("edits")?.as_array()?;
    if edits.is_empty() {
        return None;
    }
    let mut formatter = DiffFormatter::new(theme);
    for (idx, edit) in edits.iter().enumerate() {
        formatter.format_entry(idx, edit);
    }
    Some(formatter.out)
}

pub fn format_thinking_block(thinking_text: &str, theme: &Theme) -> String {
    let d = theme.dimmed;
    let mut out = String::new();
    for line in thinking_text.trim().lines() {
        out.push_str(&format!("  {d}{line}{d:#}\n"));
    }
    out.push('\n');
    out
}

fn bash_approval_details(request: &BashApproval<'_>) -> Vec<String> {
    let mut lines = vec![format!("$ {}", clean_command_paths(request.command))];
    if !request.reasons.is_empty() {
        lines.push(String::new());
        lines.extend(request.reasons.iter().map(|reason| format!("- {reason}")));
    }
    lines
}

fn risk_badge(tier: RiskTier) -> &'static str {
    match tier {
        RiskTier::ReadOnly => "[READ ONLY]",
        RiskTier::Mutating => "[MUTATING]",
        RiskTier::HighRisk => "[HIGH RISK: DESTRUCTIVE ACTION]",
    }
}

pub fn to_relative_path(raw_path: &str) -> String {
    let clean = raw_path.trim().trim_matches('"').trim_matches('\'');
    let path = Path::new(clean);
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(rel) = path.strip_prefix(&cwd)
    {
        let rel_str = rel.display().to_string();
        return if rel_str.is_empty() { ".".to_string() } else { rel_str };
    }
    if let Ok(home) = std::env::var("HOME")
        && let Ok(rel) = path.strip_prefix(Path::new(&home))
    {
        return format!("~/{}", rel.display());
    }
    clean.to_string()
}

pub fn clean_command_paths(cmd: &str) -> String {
    let mut cleaned = cmd.to_string();
    if let Ok(cwd) = std::env::current_dir()
        && let Some(cwd_str) = cwd.to_str()
        && !cwd_str.is_empty()
    {
        cleaned = cleaned.replace(&format!("{cwd_str}/"), "");
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        cleaned = cleaned.replace(&format!("{home}/"), "~/");
    }
    cleaned
}

pub fn split_shell_pipeline(cmd: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut chars = cmd.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            current.push(ch);
            continue;
        }
        if ch == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            current.push(ch);
            continue;
        }

        if !in_single_quote && !in_double_quote {
            if ch == '\n' {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                    current.clear();
                }
                continue;
            }
            if ch == '&' && chars.peek() == Some(&'&') {
                chars.next();
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    parts.push(format!("{trimmed} &&"));
                }
                current.clear();
                continue;
            }
            if ch == '|' && chars.peek() == Some(&'|') {
                chars.next();
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    parts.push(format!("{trimmed} ||"));
                }
                current.clear();
                continue;
            }
            if ch == '|' {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    parts.push(format!("{trimmed} |"));
                }
                current.clear();
                continue;
            }
            if ch == ';' {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    parts.push(format!("{trimmed};"));
                }
                current.clear();
                continue;
            }
        }

        current.push(ch);
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }

    parts
}

pub fn format_tool_args_summary(name: &str, args: &serde_json::Value) -> String {
    match name {
        "read" => {
            let raw = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
            to_relative_path(raw)
        }
        "write" => {
            let raw = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
            let rel = to_relative_path(raw);
            let bytes = args
                .get("content")
                .and_then(|c| c.as_str())
                .map(|c| c.len())
                .unwrap_or(0);
            format!("{rel} ({bytes} bytes)")
        }
        "edit" => {
            let raw = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
            let rel = to_relative_path(raw);
            let edits_count = args
                .get("edits")
                .and_then(|e| e.as_array())
                .map(|e| e.len())
                .unwrap_or(0);
            format!("{rel} ({edits_count} edits)")
        }
        "bash" => {
            let raw_cmd = args.get("command").and_then(|c| c.as_str()).unwrap_or("");
            let clean = clean_command_paths(raw_cmd);
            if clean.len() > 60 {
                format!("`{}...`", &clean[..60])
            } else {
                format!("`{clean}`")
            }
        }
        "websearch" => {
            let q = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
            format!("\"{q}\"")
        }
        "webfetch" => {
            let raw_url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
            to_relative_path(raw_url)
        }
        "ask_user" | "ask_user_question" => {
            if let Some(q) = args.get("question").and_then(|q| q.as_str()) {
                if q.len() > 60 {
                    format!("\"{}...\"", &q[..60])
                } else {
                    format!("\"{q}\"")
                }
            } else if let Some(questions) = args.get("questions").and_then(|v| v.as_array()) {
                if let Some(first) = questions
                    .first()
                    .and_then(|q| q.get("question"))
                    .and_then(|v| v.as_str())
                {
                    format!(
                        "\"{}...\" ({} questions)",
                        &first[..first.len().min(40)],
                        questions.len()
                    )
                } else {
                    format!("{} questions", questions.len())
                }
            } else {
                "".to_string()
            }
        }
        _ => "".to_string(),
    }
}

pub fn summarize_tool_output(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or("").trim();
    if first_line.len() > 60 {
        format!("{}...", &first_line[..60])
    } else if !first_line.is_empty() {
        first_line.to_string()
    } else {
        format!("{} lines", content.lines().count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_shell_pipeline() {
        let cmd = "echo 'hello' && cat Cargo.toml | grep name; ls -la";
        let parts = split_shell_pipeline(cmd);
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "echo 'hello' &&");
        assert_eq!(parts[1], "cat Cargo.toml |");
        assert_eq!(parts[2], "grep name;");
        assert_eq!(parts[3], "ls -la");
    }

    #[test]
    fn test_bash_approval_details_include_command_and_reasons() {
        let reasons = vec!["Writes output through file redirection".to_string()];
        let details = bash_approval_details(&BashApproval {
            command: "echo test > output.txt",
            tier: RiskTier::Mutating,
            reasons: &reasons,
        });

        assert_eq!(
            details,
            [
                "$ echo test > output.txt",
                "",
                "- Writes output through file redirection"
            ]
        );
    }

    #[test]
    fn test_risk_badges() {
        assert_eq!(risk_badge(RiskTier::ReadOnly), "[READ ONLY]");
        assert_eq!(risk_badge(RiskTier::Mutating), "[MUTATING]");
        assert_eq!(risk_badge(RiskTier::HighRisk), "[HIGH RISK: DESTRUCTIVE ACTION]");
    }

    #[test]
    fn test_to_relative_path() {
        let cwd = std::env::current_dir().unwrap();
        let abs = cwd.join("src/main.rs");
        let rel = to_relative_path(abs.to_str().unwrap());
        assert_eq!(rel, "src/main.rs");
    }

    #[test]
    fn test_clean_command_paths() {
        let cwd = std::env::current_dir().unwrap();
        let cwd_str = cwd.to_str().unwrap();
        let cmd = format!("cat {cwd_str}/Cargo.toml");
        let cleaned = clean_command_paths(&cmd);
        assert_eq!(cleaned, "cat Cargo.toml");
    }

    #[test]
    fn test_format_edit_diff_renders_removals_and_additions() {
        let theme = Theme::default();
        let args = serde_json::json!({
            "path": "src/main.rs",
            "edits": [
                {
                    "oldText": "let x = 1;",
                    "newText": "let x = 2;\nlet y = 3;"
                }
            ]
        });
        let diff = format_edit_diff(&args, &theme).unwrap();
        assert!(diff.contains("- let x = 1;"));
        assert!(diff.contains("+ let x = 2;"));
        assert!(diff.contains("+ let y = 3;"));
    }

    #[test]
    fn test_format_thinking_block_renders_dimmed_with_trailing_breaks() {
        let theme = Theme::default();
        let formatted = format_thinking_block("analyzing the problem\nchecking tests", &theme);
        assert!(formatted.contains("analyzing the problem"));
        assert!(formatted.contains("checking tests"));
        assert!(!formatted.contains("┌─ Thinking"));
        assert!(formatted.ends_with("\n\n"));
    }
}
