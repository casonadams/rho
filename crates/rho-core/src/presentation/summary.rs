//! Path-cleaning, tool-arg summarization, and bash-approval helpers.

use crate::args::read::DEFAULT_READ_LIMIT;
use rho_sdk::ui::BashApproval;
use rho_sdk::ui::RiskTier;
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
            let (path, range) = read_summary_parts(args);
            format!("{path}{}", range.unwrap_or_default())
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
        "websearch" | "web_search" => {
            let q = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
            format!("\"{q}\"")
        }
        "webfetch" | "web_fetch" => {
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
