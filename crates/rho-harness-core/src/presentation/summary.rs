//! Path-cleaning and tool-arg summarization helpers.

use crate::args::read::DEFAULT_READ_LIMIT;
use std::path::Path;

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

fn classify_skill(path: &Path, file_name: &str) -> Option<ReadClassification> {
    if file_name.eq_ignore_ascii_case("SKILL.md") {
        let skill_name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|f| f.to_str())
            .unwrap_or(file_name)
            .to_string();
        Some(ReadClassification::Skill { name: skill_name })
    } else {
        None
    }
}

fn classify_doc_or_resource(clean: &str, file_name: &str) -> Option<ReadClassification> {
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

pub fn classify_read_path(args: &serde_json::Value) -> Option<ReadClassification> {
    let raw = args.get("path").and_then(|path| path.as_str())?;
    let clean = raw.trim().trim_matches('"').trim_matches('\'');
    let path = Path::new(clean);
    let file_name = path.file_name()?.to_str()?;
    classify_skill(path, file_name).or_else(|| classify_doc_or_resource(clean, file_name))
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

fn format_file_mutation_summary(name: &str, args: &serde_json::Value) -> String {
    let raw = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
    let rel = to_relative_path(raw);
    if name == "write" {
        let bytes = args
            .get("content")
            .and_then(|c| c.as_str())
            .map(|c| c.len())
            .unwrap_or(0);
        format!("{rel} ({bytes} bytes)")
    } else {
        let edits_count = args
            .get("edits")
            .and_then(|e| e.as_array())
            .map(|e| e.len())
            .unwrap_or(0);
        format!("{rel} ({edits_count} edits)")
    }
}

fn format_bash_summary(args: &serde_json::Value) -> String {
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

fn format_read_summary(args: &serde_json::Value) -> String {
    if let Some(ReadClassification::Skill { name }) = classify_read_path(args) {
        format!("[skill] {name}")
    } else {
        let (path, range) = read_summary_parts(args);
        format!("{path}{}", range.unwrap_or_default())
    }
}

fn format_search_summary(name: &str, args: &serde_json::Value) -> String {
    if name == "grep" || name == "rg" {
        let pattern = args.get("pattern").and_then(|p| p.as_str()).unwrap_or("");
        let path = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
        let rel = to_relative_path(path);
        let quoted_pat = quote_cli_arg(pattern);
        if rel == "." || rel.is_empty() {
            quoted_pat
        } else {
            format!("{quoted_pat} {}", quote_cli_arg(&rel))
        }
    } else {
        format_fd_summary(args)
    }
}

fn format_fd_summary(args: &serde_json::Value) -> String {
    let pattern = args.get("pattern").and_then(|p| p.as_str()).unwrap_or("");
    let path = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
    let rel = to_relative_path(path);
    let has_path = rel != "." && !rel.is_empty();
    let has_pattern = !pattern.is_empty();
    match (has_pattern, has_path) {
        (true, true) => format!("{} {}", quote_cli_arg(pattern), quote_cli_arg(&rel)),
        (true, false) => quote_cli_arg(pattern),
        (false, true) => format!(". {}", quote_cli_arg(&rel)),
        (false, false) => ".".to_string(),
    }
}

pub fn format_tool_args_summary(name: &str, args: &serde_json::Value) -> String {
    match name {
        "read" => format_read_summary(args),
        "write" | "edit" => format_file_mutation_summary(name, args),
        "bash" => format_bash_summary(args),
        "web_search" => format!("\"{}\"", args.get("query").and_then(|q| q.as_str()).unwrap_or("")),
        "web_fetch" => to_relative_path(args.get("url").and_then(|u| u.as_str()).unwrap_or("")),
        "script" => format_script_summary(args),
        "grep" | "rg" | "fd" => format_search_summary(name, args),
        "ls" => to_relative_path(args.get("path").and_then(|p| p.as_str()).unwrap_or(".")),
        _ => "".to_string(),
    }
}

fn format_script_summary(args: &serde_json::Value) -> String {
    let steps = args.get("steps").and_then(|s| s.as_array());
    let Some(steps) = steps else {
        return String::new();
    };
    if steps.is_empty() {
        return "0 steps".to_string();
    }
    if steps.len() == 1 {
        let step = &steps[0];
        let tool = step.get("tool").and_then(|t| t.as_str()).unwrap_or("tool");
        let inner_args = step.get("args").unwrap_or(&serde_json::Value::Null);
        let inner_summary = format_tool_args_summary(tool, inner_args);
        let filter = step.get("filter").and_then(|f| f.as_str());
        let context = step.get("context").and_then(|c| c.as_u64()).map(|c| c as usize);
        let pipe = if let Some(pattern) = filter {
            let ctx = context.unwrap_or(2);
            if ctx > 0 {
                format!(" | grep -C {ctx} {pattern:?}")
            } else {
                format!(" | grep {pattern:?}")
            }
        } else {
            String::new()
        };
        if inner_summary.is_empty() {
            format!("{tool}{pipe}")
        } else {
            format!("{tool} {inner_summary}{pipe}")
        }
    } else {
        format!("({} steps)", steps.len())
    }
}

pub fn quote_cli_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    let is_safe = arg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/' | '.' | ':' | '@' | '+'));
    if is_safe {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
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
