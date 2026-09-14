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

fn format_kv_val(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => {
            let single_line = s.replace('\r', "").replace('\n', " ");
            let (preview, truncated) = truncate_preview(&single_line, 40);
            let clean = preview.replace('"', "\\\"");
            if truncated {
                format!("\"{clean}...\"")
            } else {
                format!("\"{clean}\"")
            }
        }
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            let s = serde_json::to_string(v).unwrap_or_default();
            let (preview, truncated) = truncate_preview(&s, 30);
            if truncated { format!("{preview}...") } else { s }
        }
    }
}

fn format_object_args(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut parts = Vec::new();
    for (k, v) in obj {
        parts.push(format!("{k}={}", format_kv_val(v)));
    }
    let joined = parts.join(" ");
    let (preview, truncated) = truncate_preview(&joined, 60);
    if truncated { format!("{preview}...") } else { joined }
}

fn format_single_mcp_call(server: Option<&str>, tool: &str, inner_args: Option<&serde_json::Value>) -> String {
    let target = match server {
        Some(s) if !s.is_empty() => format!("{s}:{tool}"),
        _ => tool.to_string(),
    };
    let args_str = match inner_args {
        Some(serde_json::Value::Object(map)) if !map.is_empty() => format_object_args(map),
        Some(serde_json::Value::String(s)) if !s.is_empty() => format_kv_val(&serde_json::Value::String(s.clone())),
        _ => String::new(),
    };
    if args_str.is_empty() {
        target
    } else {
        format!("{target} {args_str}")
    }
}

fn extract_mcp_call_args(args: &serde_json::Value) -> Option<serde_json::Value> {
    if let Some(inner) = args.get("args") {
        return Some(inner.clone());
    }
    if let serde_json::Value::Object(map) = args {
        let non_meta: serde_json::Map<String, serde_json::Value> = map
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "action" | "server" | "tool" | "search" | "describe"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if !non_meta.is_empty() {
            return Some(serde_json::Value::Object(non_meta));
        }
    }
    None
}

fn format_mcp_summary(args: &serde_json::Value) -> String {
    let action = args.get("action").and_then(|a| a.as_str());
    let server = args.get("server").and_then(|s| s.as_str());

    if action == Some("call") || args.get("tool").is_some() {
        let tool = args.get("tool").and_then(|t| t.as_str()).unwrap_or("");
        if !tool.is_empty() {
            let inner_args = extract_mcp_call_args(args);
            return format_single_mcp_call(server, tool, inner_args.as_ref());
        }
    }

    if action == Some("search") || args.get("search").is_some() {
        let query = args.get("search").and_then(|q| q.as_str()).unwrap_or("");
        let query_str = format_kv_val(&serde_json::Value::String(query.to_string()));
        return match server {
            Some(s) if !s.is_empty() => format!("{s}:search {query_str}"),
            _ => format!("search {query_str}"),
        };
    }

    if action == Some("describe") || args.get("describe").is_some() {
        let target = args.get("describe").and_then(|d| d.as_str()).unwrap_or("");
        return match server {
            Some(s) if !s.is_empty() => format!("{s}:describe {target}"),
            _ => format!("describe {target}"),
        };
    }

    if action == Some("status") {
        return match server {
            Some(s) if !s.is_empty() => format!("status {s}"),
            _ => "status".to_string(),
        };
    }

    if let Some(act) = action {
        return match server {
            Some(s) if !s.is_empty() => format!("{s}:{act}"),
            _ => act.to_string(),
        };
    }

    if let Some(s) = server {
        return s.to_string();
    }

    String::new()
}

fn format_mcp_script_summary(args: &serde_json::Value) -> String {
    let calls = args.get("calls").and_then(|c| c.as_array());
    match calls {
        Some(list) if list.len() == 1 => {
            let item = &list[0];
            let server = item.get("server").and_then(|s| s.as_str());
            let tool = item.get("tool").and_then(|t| t.as_str()).unwrap_or("");
            let inner_args = item.get("args");
            format_single_mcp_call(server, tool, inner_args)
        }
        Some(list) if !list.is_empty() => {
            let targets: Vec<String> = list
                .iter()
                .map(|item| {
                    let server = item.get("server").and_then(|s| s.as_str());
                    let tool = item.get("tool").and_then(|t| t.as_str()).unwrap_or("tool");
                    match server {
                        Some(s) if !s.is_empty() => format!("{s}:{tool}"),
                        _ => tool.to_string(),
                    }
                })
                .collect();
            let joined = targets.join(", ");
            let (preview, truncated) = truncate_preview(&joined, 65);
            if truncated {
                format!("[{} calls] {preview}...", list.len())
            } else {
                format!("[{} calls] {joined}", list.len())
            }
        }
        _ => "batch".to_string(),
    }
}

fn format_generic_args_summary(args: &serde_json::Value) -> String {
    if let serde_json::Value::Object(map) = args
        && !map.is_empty()
    {
        format_object_args(map)
    } else {
        String::new()
    }
}

pub fn format_tool_args_summary(name: &str, args: &serde_json::Value) -> String {
    match name {
        "read" => format_read_summary(args),
        "write" | "edit" => format_file_mutation_summary(name, args),
        "bash" => format_bash_summary(args),
        "web_search" => format!("\"{}\"", args.get("query").and_then(|q| q.as_str()).unwrap_or("")),
        "web_fetch" => to_relative_path(args.get("url").and_then(|u| u.as_str()).unwrap_or("")),
        "grep" | "rg" | "fd" => format_search_summary(name, args),
        "ls" => to_relative_path(args.get("path").and_then(|p| p.as_str()).unwrap_or(".")),
        "mcp" => format_mcp_summary(args),
        "mcpScript" => format_mcp_script_summary(args),
        _ => format_generic_args_summary(args),
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
