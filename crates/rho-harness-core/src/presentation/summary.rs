//! Path-cleaning, tool-arg summarization, and bash-approval helpers.

use super::types::{BashApproval, RiskTier};
use crate::args::read::DEFAULT_READ_LIMIT;
use std::path::Path;

pub fn bash_approval_details(request: &BashApproval) -> Vec<String> {
    let mut lines = vec![format!("$ {}", clean_command_paths(&request.command))];
    if request.tier == RiskTier::HighRisk && !request.reasons.is_empty() {
        lines.push(String::new());
        lines.extend(request.reasons.iter().map(|reason| reason.to_string()));
    }
    lines
}

pub fn approval_heading(tier: RiskTier) -> &'static str {
    match tier {
        RiskTier::HighRisk => "High-risk bash command",
        RiskTier::ReadOnly | RiskTier::Mutating => "Bash command requires approval",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadClassification {
    Skill { name: String },
    Resource { path: String },
    Docs { path: String },
}

pub fn classify_read_path(args: &serde_json::Value) -> Option<ReadClassification> {
    let raw = args.get("path").and_then(|path| path.as_str())?;
    let clean = raw.trim().trim_matches('"').trim_matches('\'');
    let path = Path::new(clean);
    let file_name = path.file_name()?.to_str()?;

    if file_name.eq_ignore_ascii_case("SKILL.md") {
        let skill_name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|f| f.to_str())
            .unwrap_or(file_name)
            .to_string();
        return Some(ReadClassification::Skill { name: skill_name });
    }

    if file_name == "AGENTS.md"
        || file_name == "AGENTS.override.md"
        || file_name == "CLAUDE.md"
        || file_name == "CLAUDE.MD"
    {
        return Some(ReadClassification::Resource {
            path: to_relative_path(clean),
        });
    }

    if file_name.eq_ignore_ascii_case("README.md") || clean.contains("docs/") || clean.contains("examples/") {
        return Some(ReadClassification::Docs {
            path: to_relative_path(clean),
        });
    }

    None
}

pub fn read_summary_parts(args: &serde_json::Value) -> (String, Option<String>) {
    let raw = args.get("path").and_then(|path| path.as_str()).unwrap_or("");
    let path = to_relative_path(raw);
    if args.get("offset").is_none() && args.get("limit").is_none() {
        return (path, None);
    }
    let start = args
        .get("offset")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1)
        .max(1);
    let limit = args
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(DEFAULT_READ_LIMIT as u64);
    let end = start.saturating_add(limit.saturating_sub(1));
    (path, Some(format!(":{start}-{end}")))
}

pub fn format_tool_args_summary(name: &str, args: &serde_json::Value) -> String {
    match name {
        "read" => {
            if let Some(ReadClassification::Skill { name }) = classify_read_path(args) {
                format!("[skill] {name}")
            } else {
                let (path, range) = read_summary_parts(args);
                format!("{path}{}", range.unwrap_or_default())
            }
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
            let (preview, was_truncated) = truncate_preview(&clean, 60);
            let cmd_str = if was_truncated { format!("{preview}...") } else { clean };
            if let Some(timeout) = args
                .get("timeout")
                .and_then(|t| t.as_u64().or_else(|| t.as_f64().map(|f| f as u64)))
            {
                format!("{cmd_str} (timeout {timeout}s)")
            } else {
                cmd_str
            }
        }
        "web_search" => {
            let q = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
            format!("\"{q}\"")
        }
        "web_fetch" => {
            let raw_url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");
            to_relative_path(raw_url)
        }
        "grep" | "rg" => {
            let pattern = args.get("pattern").and_then(|p| p.as_str()).unwrap_or("");
            let path = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
            let rel = to_relative_path(path);
            format!("/{pattern}/ in {rel}")
        }
        "fd" => {
            let pattern = args.get("pattern").and_then(|p| p.as_str()).unwrap_or("");
            let path = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
            let rel = to_relative_path(path);
            if pattern.is_empty() {
                rel
            } else {
                format!("{pattern} in {rel}")
            }
        }
        "ls" => {
            let path = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
            to_relative_path(path)
        }
        _ => "".to_string(),
    }
}

/// Full-fidelity tool input for approval prompts: unlike the tool-line
/// summary there is no truncation, since the user is approving exactly this
/// input. Unknown tools return an empty string (callers fall back to JSON).
pub fn format_tool_args_full(name: &str, args: &serde_json::Value) -> String {
    match name {
        "bash" => clean_command_paths(args.get("command").and_then(|c| c.as_str()).unwrap_or("")),
        "read" | "write" | "edit" => to_relative_path(args.get("path").and_then(|p| p.as_str()).unwrap_or("")),
        "web_search" => args.get("query").and_then(|q| q.as_str()).unwrap_or("").to_string(),
        "web_fetch" => to_relative_path(args.get("url").and_then(|u| u.as_str()).unwrap_or("")),
        _ => String::new(),
    }
}

fn truncate_preview(text: &str, limit: usize) -> (&str, bool) {
    if let Some((idx, _)) = text.char_indices().nth(limit) {
        (&text[..idx], true)
    } else {
        (text, false)
    }
}

pub fn summarize_tool_output(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or("").trim();
    let (preview, was_truncated) = truncate_preview(first_line, 60);
    if was_truncated {
        format!("{preview}...")
    } else if !first_line.is_empty() {
        first_line.to_string()
    } else {
        format!("{} lines", content.lines().count())
    }
}

#[cfg(test)]
mod tests;
