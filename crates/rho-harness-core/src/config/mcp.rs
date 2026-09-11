use super::types::{McpConfig, McpExposureMode, McpServerConfig, McpTransportKind};
use crate::error::{AppError, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

fn expand_env_vars(s: &str) -> String {
    if let Some(var_name) = s.strip_prefix("env:") {
        if let Ok(val) = std::env::var(var_name) {
            return val;
        }
        return s.to_string();
    }

    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut var_name = String::new();
            let mut closed = false;
            for inner in chars.by_ref() {
                if inner == '}' {
                    closed = true;
                    break;
                }
                var_name.push(inner);
            }
            if closed {
                if let Ok(val) = std::env::var(&var_name) {
                    out.push_str(&val);
                } else {
                    out.push_str(&format!("${{{var_name}}}"));
                }
            } else {
                out.push_str("${");
                out.push_str(&var_name);
            }
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

fn parse_single_server_json(obj: &serde_json::Map<String, Value>) -> McpServerConfig {
    let command = obj.get("command").and_then(|v| v.as_str()).map(expand_env_vars);
    let args = parse_string_vec(obj.get("args"));
    let env = parse_string_map(obj.get("env"));
    let enabled = obj.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    let url = obj.get("url").and_then(|v| v.as_str()).map(expand_env_vars);
    let transport = parse_transport_kind(obj.get("transport"));
    let headers = parse_string_map(obj.get("headers"));
    let mode = parse_exposure_mode(obj.get("mode"));
    let include_tools = obj
        .get("includeTools")
        .or_else(|| obj.get("include_tools"))
        .map(|v| parse_string_vec(Some(v)))
        .filter(|v| !v.is_empty());
    let exclude_tools = obj
        .get("excludeTools")
        .or_else(|| obj.get("exclude_tools"))
        .map(|v| parse_string_vec(Some(v)))
        .filter(|v| !v.is_empty());
    let timeout_seconds = obj
        .get("timeout")
        .or_else(|| obj.get("timeout_seconds"))
        .and_then(|v| v.as_u64());

    McpServerConfig {
        command,
        args,
        env,
        enabled,
        url,
        transport,
        headers,
        mode,
        include_tools,
        exclude_tools,
        timeout_seconds,
    }
}

pub fn parse_mcp_json_str(content: &str) -> Result<McpConfig> {
    let root: Value =
        serde_json::from_str(content).map_err(|e| AppError::Config(format!("Failed to parse .mcp.json: {e}")))?;

    let server_map = if let Some(mcp_servers) = root.get("mcpServers").and_then(|v| v.as_object()) {
        mcp_servers
    } else if let Some(obj) = root.as_object() {
        obj
    } else {
        return Err(AppError::Config(
            "Invalid .mcp.json format: expected an object with 'mcpServers' or server entries".to_string(),
        ));
    };

    let mut servers = BTreeMap::new();
    for (name, val) in server_map {
        if let Some(obj) = val.as_object() {
            servers.insert(name.clone(), parse_single_server_json(obj));
        }
    }

    Ok(McpConfig { enabled: true, servers })
}

pub fn load_project_mcp_config(workspace_dir: &Path) -> Result<Option<McpConfig>> {
    let candidate = workspace_dir.join(".mcp.json");
    if candidate.is_file() {
        let content = std::fs::read_to_string(&candidate)
            .map_err(|e| AppError::Config(format!("Failed to read {}: {e}", candidate.display())))?;
        return parse_mcp_json_str(&content).map(Some);
    }

    let rho_candidate = workspace_dir.join(".rho").join("mcp.json");
    if rho_candidate.is_file() {
        let content = std::fs::read_to_string(&rho_candidate)
            .map_err(|e| AppError::Config(format!("Failed to read {}: {e}", rho_candidate.display())))?;
        return parse_mcp_json_str(&content).map(Some);
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mcp_servers_wrapper() {
        let json = r#"{
            "mcpServers": {
                "github": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-github"],
                    "env": { "GITHUB_TOKEN": "test_tok" }
                },
                "remote": {
                    "url": "https://example.com/mcp",
                    "transport": "streamable-http",
                    "mode": "gateway",
                    "headers": { "Authorization": "Bearer abc" }
                }
            }
        }"#;

        let parsed = parse_mcp_json_str(json).unwrap();
        assert_eq!(parsed.servers.len(), 2);

        let gh = &parsed.servers["github"];
        assert_eq!(gh.command.as_deref(), Some("npx"));
        assert_eq!(gh.args.len(), 2);
        assert_eq!(gh.env["GITHUB_TOKEN"], "test_tok");
        assert_eq!(gh.resolved_transport(), McpTransportKind::Stdio);

        let remote = &parsed.servers["remote"];
        assert_eq!(remote.url.as_deref(), Some("https://example.com/mcp"));
        assert_eq!(remote.transport, Some(McpTransportKind::StreamableHttp));
        assert_eq!(remote.mode, Some(McpExposureMode::Gateway));
        assert_eq!(remote.headers["Authorization"], "Bearer abc");
        assert!(remote.is_remote());
    }

    #[test]
    fn test_parse_flat_mcp_json() {
        let json = r#"{
            "filesystem": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
            }
        }"#;

        let parsed = parse_mcp_json_str(json).unwrap();
        assert_eq!(parsed.servers.len(), 1);
        assert!(parsed.servers.contains_key("filesystem"));
    }

    #[test]
    fn test_env_expansion() {
        unsafe {
            std::env::set_var("TEST_RHO_VAR", "expanded_value");
        }

        let json = r#"{
            "mcpServers": {
                "test": {
                    "command": "echo",
                    "args": ["${TEST_RHO_VAR}"],
                    "headers": { "X-Val": "env:TEST_RHO_VAR" }
                }
            }
        }"#;

        let parsed = parse_mcp_json_str(json).unwrap();
        let s = &parsed.servers["test"];
        assert_eq!(s.args[0], "expanded_value");
        assert_eq!(s.headers["X-Val"], "expanded_value");
    }
}
