use crate::permission::policy::{config_dir, protected_config_files};
use rho_harness_core::workspace::{canonicalize_path, clean_path_arg, resolve_path};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const READ_ONLY_PATH_TOOLS: &[&str] = &["read", "grep", "find", "ls", "fd", "rg"];
pub const PATH_BEARING_TOOLS: &[&str] = &["read", "grep", "find", "ls", "fd", "rg", "write", "edit"];
pub const SAFE_SYSTEM_PATHS: &[&str] = &["/dev/null", "/dev/stdin", "/dev/stdout", "/dev/stderr"];

const CREDENTIAL_FILE: &str = "auth.json";
const INFRASTRUCTURE_SUBDIRS: &[&str] = &["prompts", "skills"];
const PROTECTED_DIR_NAMES: &[&str] = &[".git", ".rho"];

pub fn is_safe_system_path(path: &str) -> bool {
    SAFE_SYSTEM_PATHS.contains(&path)
}

pub fn resolve_tool_path(raw: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    resolve_path(raw, cwd.unwrap_or(Path::new(".")))
}

pub fn path_policy_values(raw: &str, cwd: Option<&Path>) -> Vec<String> {
    let Some(absolute) = resolve_tool_path(raw, cwd) else {
        return Vec::new();
    };
    let mut values = vec![absolute.to_string_lossy().into_owned()];
    if let Some(relative) = cwd.and_then(|cwd| relative_to(&absolute, cwd)) {
        values.push(relative);
    }
    values
}

fn relative_to(absolute: &Path, cwd: &Path) -> Option<String> {
    let relative = absolute.strip_prefix(canonicalize_path(cwd)).ok()?;
    (!relative.as_os_str().is_empty()).then(|| relative.to_string_lossy().into_owned())
}

pub fn is_path_outside_working_dir(raw: &str, cwd: Option<&Path>) -> bool {
    if is_safe_system_path(clean_path_arg(raw)) {
        return false;
    }
    let Some(cwd) = cwd else {
        return true;
    };
    resolve_tool_path(raw, Some(cwd)).is_none_or(|absolute| !absolute.starts_with(canonicalize_path(cwd)))
}

pub fn is_protected_path(raw: &str, cwd: Option<&Path>) -> bool {
    if is_safe_system_path(clean_path_arg(raw)) {
        return false;
    }
    let Some(absolute) = resolve_tool_path(raw, cwd) else {
        return false;
    };
    let protected_dir = absolute.components().any(|component| {
        PROTECTED_DIR_NAMES
            .iter()
            .any(|name| component.as_os_str().eq_ignore_ascii_case(name))
    });
    let env_file = absolute
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().starts_with(".env"));
    protected_dir || env_file || is_config_file(&absolute, cwd)
}

fn is_config_file(absolute: &Path, cwd: Option<&Path>) -> bool {
    protected_config_files(cwd)
        .iter()
        .any(|file| canonicalize_path(file) == absolute)
}

pub fn is_credential_path(raw: &str, cwd: Option<&Path>) -> bool {
    resolve_tool_path(raw, cwd).is_some_and(|absolute| is_credential_file(&absolute))
}

pub fn is_credential_file(absolute: &Path) -> bool {
    config_dir().is_some_and(|dir| canonicalize_path(&dir.join(CREDENTIAL_FILE)) == absolute)
}

pub fn is_infrastructure_read(tool: &str, raw: &str, cwd: Option<&Path>) -> bool {
    if !READ_ONLY_PATH_TOOLS.contains(&tool) {
        return false;
    }
    let (Some(dir), Some(absolute)) = (config_dir(), resolve_tool_path(raw, cwd)) else {
        return false;
    };
    INFRASTRUCTURE_SUBDIRS
        .iter()
        .any(|sub| absolute.starts_with(canonicalize_path(&dir.join(sub))))
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
