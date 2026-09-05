use crate::permission::matcher::expand_home;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

pub const READ_ONLY_PATH_TOOLS: &[&str] = &["read", "grep", "find", "ls", "fd", "rg"];
pub const PATH_BEARING_TOOLS: &[&str] = &["read", "grep", "find", "ls", "fd", "rg", "write", "edit"];
pub const SAFE_SYSTEM_PATHS: &[&str] = &["/dev/null", "/dev/stdin", "/dev/stdout", "/dev/stderr"];

pub fn is_safe_system_path(path: &str) -> bool {
    SAFE_SYSTEM_PATHS.contains(&path)
}

pub fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            c => normalized.push(c),
        }
    }
    normalized
}

pub fn canonical_path(path_str: &str, cwd: &Path) -> PathBuf {
    let expanded = expand_home(path_str.trim());
    let path = Path::new(&expanded);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    absolute.canonicalize().unwrap_or_else(|_| normalize_lexical(&absolute))
}

pub fn is_path_within_dir(path: &Path, directory: &Path) -> bool {
    let norm_path = normalize_lexical(path);
    let norm_dir = normalize_lexical(directory);
    norm_path.starts_with(&norm_dir)
}

pub fn is_path_outside_working_dir(path_str: &str, cwd: Option<&Path>) -> bool {
    let trimmed = path_str.trim();
    if is_safe_system_path(trimmed) {
        return false;
    }
    let Some(cwd) = cwd else {
        return true;
    };
    let canonical = canonical_path(trimmed, cwd);
    let canonical_cwd = cwd.canonicalize().unwrap_or_else(|_| normalize_lexical(cwd));
    !is_path_within_dir(&canonical, &canonical_cwd)
}

pub fn is_infrastructure_read(tool: &str, path_str: &str, cwd: Option<&Path>) -> bool {
    if !READ_ONLY_PATH_TOOLS.contains(&tool) {
        return false;
    }
    let Some(config_dir) = crate::permission::policy::config_dir() else {
        return false;
    };
    let base_cwd = cwd.unwrap_or(Path::new("."));
    let target = canonical_path(path_str, base_cwd);
    let infra = canonical_path(&config_dir.to_string_lossy(), base_cwd);
    is_path_within_dir(&target, &infra)
}

pub fn path_policy_values(path_str: &str, cwd: Option<&Path>) -> Vec<String> {
    let trimmed = path_str.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut values = Vec::new();
    let expanded = expand_home(trimmed);
    values.push(expanded.clone());

    if let Some(cwd) = cwd {
        add_canonical_and_relative(&expanded, cwd, &mut values);
    }
    values
}

fn add_canonical_and_relative(expanded: &str, cwd: &Path, values: &mut Vec<String>) {
    let canonical = canonical_path(expanded, cwd);
    let canonical_str = canonical.to_string_lossy().to_string();
    if !values.contains(&canonical_str) {
        values.push(canonical_str);
    }
    let canonical_cwd = cwd.canonicalize().unwrap_or_else(|_| normalize_lexical(cwd));
    if let Ok(relative) = canonical.strip_prefix(&canonical_cwd) {
        let rel_str = relative.to_string_lossy().to_string();
        if !rel_str.is_empty() && !values.contains(&rel_str) {
            values.push(rel_str);
        }
    }
}

pub fn extract_tool_path(tool: &str, args: &Value) -> Option<String> {
    if !PATH_BEARING_TOOLS.contains(&tool) {
        return None;
    }
    args.get("path")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(String::from)
}

pub fn extract_mcp_path(args: &Value) -> Option<String> {
    let obj = args.as_object()?;
    let arguments = obj.get("arguments")?.as_object()?;
    arguments
        .get("path")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(String::from)
}

pub fn extract_mcp_targets(args: &Value) -> Vec<String> {
    let Some(obj) = args.as_object() else {
        return Vec::new();
    };
    let server = obj.get("server").and_then(Value::as_str).unwrap_or("").trim();
    let tool = obj.get("tool").and_then(Value::as_str).unwrap_or("").trim();
    let mut targets = Vec::new();
    if !server.is_empty() && !tool.is_empty() {
        targets.push(format!("{server}:{tool}"));
    }
    if !server.is_empty() {
        targets.push(server.to_string());
    }
    if server.is_empty() && !tool.is_empty() {
        targets.push(tool.to_string());
    }
    targets
}
