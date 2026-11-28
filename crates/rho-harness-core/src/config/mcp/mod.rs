use super::types::{
    McpConfig, McpDirectTools, McpExposureMode, McpLifecycleMode, McpOAuthConfig, McpServerConfig, McpTransportKind,
    global_mcp_home,
};
use crate::error::{AppError, Result};
use crate::fs::write_atomic;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn expand_bracketed_var(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut var_name = String::new();
    for inner in chars.by_ref() {
        if inner == '}' {
            return std::env::var(&var_name).unwrap_or_else(|_| format!("${{{var_name}}}"));
        }
        var_name.push(inner);
    }
    format!("${{{var_name}")
}

fn expand_env_vars(s: &str) -> String {
    if let Some(var_name) = s.strip_prefix("env:") {
        return std::env::var(var_name).unwrap_or_else(|_| s.to_string());
    }

    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' && chars.peek() == Some(&'{') {
            chars.next();
            out.push_str(&expand_bracketed_var(&mut chars));
        } else {
            out.push(ch);
        }
    }

    out
}

fn parse_string_vec(val: Option<&Value>) -> Vec<String> {
    val.and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(expand_env_vars)).collect())
        .unwrap_or_default()
}

fn parse_string_map(val: Option<&Value>) -> BTreeMap<String, String> {
    val.and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), expand_env_vars(s))))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_exposure_mode(val: Option<&Value>) -> Option<McpExposureMode> {
    match val.and_then(|v| v.as_str()) {
        Some("direct") => Some(McpExposureMode::Direct),
        Some("gateway") => Some(McpExposureMode::Gateway),
        Some("auto") => Some(McpExposureMode::Auto),
        _ => None,
    }
}

fn parse_transport_kind(val: Option<&Value>) -> Option<McpTransportKind> {
    match val.and_then(|v| v.as_str()) {
        Some("stdio") => Some(McpTransportKind::Stdio),
        Some("streamable-http" | "streamable_http" | "http") => Some(McpTransportKind::StreamableHttp),
        Some("sse") => Some(McpTransportKind::Sse),
        _ => None,
    }
}

fn parse_lifecycle_mode(val: Option<&Value>) -> Option<McpLifecycleMode> {
    match val.and_then(|v| v.as_str()) {
        Some("lazy") => Some(McpLifecycleMode::Lazy),
        Some("eager") => Some(McpLifecycleMode::Eager),
        Some("keep-alive" | "keep_alive") => Some(McpLifecycleMode::KeepAlive),
        Some("lazy-keep-alive" | "lazy_keep_alive") => Some(McpLifecycleMode::LazyKeepAlive),
        _ => None,
    }
}

fn parse_direct_tools(val: Option<&Value>) -> Option<McpDirectTools> {
    match val {
        Some(Value::Bool(b)) => Some(McpDirectTools::All(*b)),
        Some(Value::String(s)) if s == "search" => Some(McpDirectTools::Search(s.clone())),
        Some(Value::Array(arr)) => {
            let tools: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            Some(McpDirectTools::List(tools))
        }
        _ => None,
    }
}

fn parse_search_keywords(val: Option<&Value>) -> BTreeMap<String, Vec<String>> {
    val.and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| {
                    let keywords = parse_string_vec(Some(v));
                    if keywords.is_empty() {
                        None
                    } else {
                        Some((k.clone(), keywords))
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_scopes(val: Option<&Value>) -> Vec<String> {
    match val {
        Some(Value::String(s)) => expand_env_vars(s).split_whitespace().map(str::to_string).collect(),
        other => parse_string_vec(other),
    }
}

type JsonObject = serde_json::Map<String, Value>;

/// The first of `keys` present, so camelCase and snake_case spellings share one lookup.
fn first<'a>(obj: &'a JsonObject, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| obj.get(*key))
}

fn text(obj: &JsonObject, keys: &[&str]) -> Option<String> {
    first(obj, keys).and_then(Value::as_str).map(expand_env_vars)
}

fn non_empty_list(val: Option<&Value>) -> Option<Vec<String>> {
    Some(parse_string_vec(val)).filter(|list| !list.is_empty())
}

fn parse_oauth(val: Option<&Value>) -> Option<McpOAuthConfig> {
    let obj = val?.as_object()?;
    Some(McpOAuthConfig {
        client_id: text(obj, &["clientId", "client_id"]),
        client_secret: text(obj, &["clientSecret", "client_secret"]),
        scopes: parse_scopes(first(obj, &["scopes", "scope"])),
        redirect_port: first(obj, &["redirectPort", "redirect_port"])
            .and_then(Value::as_u64)
            .and_then(|p| u16::try_from(p).ok()),
        redirect_uri: text(obj, &["redirectUri", "redirect_uri"]),
        authorization_params: parse_string_map(first(obj, &["authorizationParams", "authorization_params"])),
    })
}

fn parse_enabled(obj: &JsonObject) -> bool {
    let enabled = obj.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    let disabled = obj.get("disabled").and_then(Value::as_bool).unwrap_or(false);
    enabled && !disabled
}

fn parse_single_server_json(obj: &JsonObject) -> McpServerConfig {
    McpServerConfig {
        command: text(obj, &["command"]),
        args: parse_string_vec(obj.get("args")),
        env: parse_string_map(obj.get("env")),
        enabled: parse_enabled(obj),
        url: text(obj, &["url"]),
        transport: parse_transport_kind(first(obj, &["transport", "type"])),
        headers: parse_string_map(obj.get("headers")),
        mode: parse_exposure_mode(obj.get("mode")),
        include_tools: non_empty_list(first(obj, &["includeTools", "include_tools"])),
        exclude_tools: non_empty_list(first(obj, &["excludeTools", "exclude_tools"])),
        timeout_seconds: first(obj, &["timeout", "timeout_seconds"]).and_then(Value::as_u64),
        lifecycle: parse_lifecycle_mode(obj.get("lifecycle")),
        direct_tools: parse_direct_tools(first(obj, &["directTools", "direct_tools"])),
        idle_timeout_seconds: first(obj, &["idleTimeout", "idle_timeout", "idle_timeout_seconds"])
            .and_then(Value::as_u64),
        search_keywords: parse_search_keywords(first(obj, &["searchKeywords", "search_keywords"])),
        oauth: parse_oauth(obj.get("oauth")),
        instructions: obj.get("instructions").and_then(Value::as_bool),
    }
}

/// The servers an `mcp.json` defines, under `mcpServers` or at the top level.
pub fn parse_mcp_json_str(content: &str) -> Result<McpConfig> {
    let root: Value =
        serde_json::from_str(content).map_err(|e| AppError::Config(format!("Failed to parse .mcp.json: {e}")))?;
    let server_map = root
        .get("mcpServers")
        .and_then(Value::as_object)
        .or_else(|| root.as_object())
        .ok_or_else(|| {
            AppError::Config(
                "Invalid .mcp.json format: expected an object with 'mcpServers' or server entries".to_string(),
            )
        })?;
    let servers = server_map
        .iter()
        .filter_map(|(name, val)| Some((name.clone(), parse_single_server_json(val.as_object()?))))
        .collect();
    Ok(McpConfig {
        servers,
        ..McpConfig::default()
    })
}

fn load_first_mcp_candidate(candidates: &[PathBuf]) -> Result<Option<McpConfig>> {
    for path in candidates {
        if path.is_file() {
            let content = std::fs::read_to_string(path)
                .map_err(|e| AppError::Config(format!("Failed to read {}: {e}", path.display())))?;
            return parse_mcp_json_str(&content).map(Some);
        }
    }
    Ok(None)
}

pub fn load_global_mcp_config(config_dir: &Path) -> Result<Option<McpConfig>> {
    let home = global_mcp_home();
    load_first_mcp_candidate(&[
        home.join(".config").join("mcp").join("mcp.json"),
        home.join(".agents").join("mcp.json"),
        home.join(".claude").join("mcp.json"),
        home.join(".claude.json"),
        config_dir.join("mcp.json"),
    ])
}

pub fn load_project_mcp_config(workspace_dir: &Path) -> Result<Option<McpConfig>> {
    load_first_mcp_candidate(&[
        workspace_dir.join(".agents").join("mcp.json"),
        workspace_dir.join(".claude").join("mcp.json"),
        workspace_dir.join(".mcp.json"),
    ])
}

fn read_mcp_root(path: &Path) -> Result<JsonObject> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(JsonObject::new()),
        Err(e) => return Err(AppError::Config(format!("Failed to read {}: {e}", path.display()))),
    };
    if content.trim().is_empty() {
        return Ok(JsonObject::new());
    }
    match serde_json::from_str(&content) {
        Ok(Value::Object(root)) => Ok(root),
        Ok(_) => Err(AppError::Config(format!(
            "{} must contain a JSON object",
            path.display()
        ))),
        Err(e) => Err(AppError::Config(format!("Failed to parse {}: {e}", path.display()))),
    }
}

fn write_mcp_root(path: &Path, root: &JsonObject) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(root).map_err(|e| AppError::Config(e.to_string()))?;
    bytes.push(b'\n');
    write_atomic(path, &bytes, 0o600).map_err(|e| AppError::Config(format!("Failed to write {}: {e}", path.display())))
}

fn mcp_servers_entry<'a>(root: &'a mut JsonObject, path: &Path) -> Result<&'a mut JsonObject> {
    root.entry("mcpServers")
        .or_insert_with(|| Value::Object(JsonObject::new()))
        .as_object_mut()
        .ok_or_else(|| AppError::Config(format!("'mcpServers' in {} must be a JSON object", path.display())))
}

pub fn write_mcp_server_json(target_path: &Path, name: &str, server: &McpServerConfig) -> Result<()> {
    let server_val = serde_json::to_value(server)
        .map_err(|e| AppError::Config(format!("Failed to serialize MCP server config: {e}")))?;
    let mut root = read_mcp_root(target_path)?;
    mcp_servers_entry(&mut root, target_path)?.insert(name.to_string(), server_val);
    write_mcp_root(target_path, &root)
}

pub fn remove_mcp_server_json(target_path: &Path, name: &str) -> Result<()> {
    if !target_path.is_file() {
        return Err(AppError::Config(format!(
            "MCP configuration file {} does not exist",
            target_path.display()
        )));
    }
    let mut root = read_mcp_root(target_path)?;
    root.get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .and_then(|servers| servers.remove(name))
        .ok_or_else(|| {
            AppError::Config(format!(
                "MCP server '{name}' is not configured in {}",
                target_path.display()
            ))
        })?;
    write_mcp_root(target_path, &root)
}

#[cfg(test)]
mod tests;
