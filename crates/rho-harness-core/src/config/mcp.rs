use super::types::{
    McpConfig, McpDirectTools, McpExposureMode, McpLifecycleMode, McpServerConfig, McpTransportKind,
    default_idle_timeout_seconds, dirs_fallback,
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
    let lifecycle = parse_lifecycle_mode(obj.get("lifecycle"));
    let direct_tools = parse_direct_tools(obj.get("directTools").or_else(|| obj.get("direct_tools")));
    let idle_timeout_seconds = obj
        .get("idleTimeout")
        .or_else(|| obj.get("idle_timeout"))
        .or_else(|| obj.get("idle_timeout_seconds"))
        .and_then(|v| v.as_u64());
    let search_keywords = parse_search_keywords(obj.get("searchKeywords").or_else(|| obj.get("search_keywords")));

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
        lifecycle,
        direct_tools,
        idle_timeout_seconds,
        search_keywords,
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

    let defer_threshold = root
        .get("defer_threshold")
        .or_else(|| root.get("deferThreshold"))
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(10);

    let idle_timeout_seconds = root
        .get("idle_timeout_seconds")
        .or_else(|| root.get("idleTimeout"))
        .or_else(|| root.get("idle_timeout"))
        .and_then(|v| v.as_u64())
        .unwrap_or_else(default_idle_timeout_seconds);

    Ok(McpConfig {
        enabled: true,
        defer_threshold,
        idle_timeout_seconds,
        servers,
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
    load_first_mcp_candidate(&[
        dirs_fallback().join(".config").join("mcp").join("mcp.json"),
        dirs_fallback().join(".agents").join("mcp.json"),
        dirs_fallback().join(".claude").join("mcp.json"),
        dirs_fallback().join(".claude.json"),
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

fn read_mcp_root(path: &Path) -> Result<serde_json::Map<String, Value>> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(serde_json::Map::new()),
        Err(e) => return Err(AppError::Config(format!("Failed to read {}: {e}", path.display()))),
    };
    if content.trim().is_empty() {
        return Ok(serde_json::Map::new());
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

fn write_mcp_root(path: &Path, root: &serde_json::Map<String, Value>) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(root).map_err(|e| AppError::Config(e.to_string()))?;
    bytes.push(b'\n');
    write_atomic(path, &bytes, 0o600).map_err(|e| AppError::Config(format!("Failed to write {}: {e}", path.display())))
}

fn mcp_servers_entry<'a>(
    root: &'a mut serde_json::Map<String, Value>,
    path: &Path,
) -> Result<&'a mut serde_json::Map<String, Value>> {
    root.entry("mcpServers")
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
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

pub fn remove_mcp_server_json(target_path: &Path, name: &str) -> Result<McpServerConfig> {
    if !target_path.is_file() {
        return Err(AppError::Config(format!(
            "MCP configuration file {} does not exist",
            target_path.display()
        )));
    }
    let mut root = read_mcp_root(target_path)?;
    let removed = root
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .and_then(|servers| servers.remove(name))
        .ok_or_else(|| {
            AppError::Config(format!(
                "MCP server '{name}' is not configured in {}",
                target_path.display()
            ))
        })?;
    let parsed_server = parse_single_server_json(
        removed
            .as_object()
            .ok_or_else(|| AppError::Config(format!("Invalid MCP server entry for '{name}'")))?,
    );
    write_mcp_root(target_path, &root)?;
    Ok(parsed_server)
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

    #[test]
    fn test_write_and_remove_mcp_server_json() {
        let temp_dir = std::env::temp_dir().join(format!("rho_mcp_test_{}", uuid::Uuid::new_v4()));
        let mcp_path = temp_dir.join("mcp.json");

        let server = McpServerConfig::stdio("echo", vec!["hello".to_string()]);
        write_mcp_server_json(&mcp_path, "echo_srv", &server).unwrap();

        let parsed = parse_mcp_json_str(&std::fs::read_to_string(&mcp_path).unwrap()).unwrap();
        assert!(parsed.servers.contains_key("echo_srv"));

        let removed = remove_mcp_server_json(&mcp_path, "echo_srv").unwrap();
        assert_eq!(removed.command.as_deref(), Some("echo"));

        let after_removal = parse_mcp_json_str(&std::fs::read_to_string(&mcp_path).unwrap()).unwrap();
        assert!(!after_removal.servers.contains_key("echo_srv"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    fn temp_mcp_file(content: Option<&str>) -> PathBuf {
        let path = crate::test_support::created_temp_dir().join("mcp.json");
        if let Some(content) = content {
            std::fs::write(&path, content).unwrap();
        }
        path
    }

    fn read_root(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    enum ServerEdit {
        Write(&'static str),
        Remove(&'static str),
    }

    const SERVER_EDIT_CASES: &[(&str, &str, ServerEdit, &[&str])] = &[
        (
            "write_preserves_other_keys",
            r#"{"numStartups": 3, "mcpServers": {"old": {"command": "x"}}}"#,
            ServerEdit::Write("new"),
            &["new", "old"],
        ),
        (
            "write_adds_servers_key_instead_of_using_root_as_map",
            r#"{"numStartups": 3}"#,
            ServerEdit::Write("new"),
            &["new"],
        ),
        (
            "remove_preserves_other_keys",
            r#"{"numStartups": 3, "mcpServers": {"a": {"command": "x"}, "b": {"command": "y"}}}"#,
            ServerEdit::Remove("a"),
            &["b"],
        ),
    ];

    #[test]
    fn write_and_remove_mcp_server_preserve_other_keys() {
        for (name, initial, edit, expected) in SERVER_EDIT_CASES {
            let path = temp_mcp_file(Some(*initial));
            match edit {
                ServerEdit::Write(server) => {
                    write_mcp_server_json(&path, server, &McpServerConfig::stdio("echo", vec![])).unwrap();
                }
                ServerEdit::Remove(server) => {
                    remove_mcp_server_json(&path, server).unwrap();
                }
            }
            let root = read_root(&path);
            assert_eq!(root["numStartups"], 3, "{name}");
            let mut names: Vec<&str> = root["mcpServers"]
                .as_object()
                .map(|servers| servers.keys().map(String::as_str).collect())
                .unwrap_or_default();
            names.sort_unstable();
            assert_eq!(names, *expected, "{name}");
            for server in *expected {
                assert!(root["mcpServers"][*server].is_object(), "{name}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_write_mcp_server_replaces_permissive_file_with_private_mode() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_mcp_file(Some("{}"));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_mcp_server_json(&path, "new", &McpServerConfig::stdio("echo", vec![])).unwrap();

        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    fn project_with_mcp_files(files: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = crate::test_support::created_temp_dir();
        for (relative, server) in files {
            let path = dir.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                path,
                format!(r#"{{"mcpServers": {{"{server}": {{"command": "echo"}}}}}}"#),
            )
            .unwrap();
        }
        dir
    }

    #[test]
    fn test_project_mcp_precedence_agents_over_root() {
        let dir = project_with_mcp_files(&[(".agents/mcp.json", "agents_srv"), (".mcp.json", "root_srv")]);
        let config = load_project_mcp_config(&dir).unwrap().unwrap();
        assert!(config.servers.contains_key("agents_srv"));
        assert!(!config.servers.contains_key("root_srv"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_project_mcp_fallback_to_root() {
        let dir = project_with_mcp_files(&[(".mcp.json", "root_srv")]);
        let config = load_project_mcp_config(&dir).unwrap().unwrap();
        assert!(config.servers.contains_key("root_srv"));

        let claude = project_with_mcp_files(&[(".claude/mcp.json", "claude_srv"), (".mcp.json", "root_srv")]);
        let config_claude = load_project_mcp_config(&claude).unwrap().unwrap();
        assert!(config_claude.servers.contains_key("claude_srv"));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&claude);
    }

    #[test]
    fn test_project_mcp_ignores_legacy_rho() {
        let dir = project_with_mcp_files(&[(".rho/mcp.json", "rho_srv")]);
        assert!(load_project_mcp_config(&dir).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_modernized_mcp_fields() {
        let json = r#"{
            "idleTimeout": 300,
            "mcpServers": {
                "db": {
                    "command": "node",
                    "args": ["server.js"],
                    "lifecycle": "keep-alive",
                    "idleTimeout": 120,
                    "directTools": ["query", "schema"],
                    "searchKeywords": {
                        "query": ["sql", "select"],
                        "*": ["database"]
                    }
                },
                "searcher": {
                    "command": "python",
                    "args": ["search.py"],
                    "directTools": "search",
                    "lifecycle": "lazy-keep-alive"
                },
                "all_direct": {
                    "command": "echo",
                    "directTools": true
                }
            }
        }"#;

        let parsed = parse_mcp_json_str(json).unwrap();
        assert_eq!(parsed.idle_timeout_seconds, 300);

        let db = &parsed.servers["db"];
        assert_eq!(db.lifecycle, Some(McpLifecycleMode::KeepAlive));
        assert_eq!(db.idle_timeout_seconds, Some(120));
        assert_eq!(
            db.direct_tools,
            Some(McpDirectTools::List(vec!["query".to_string(), "schema".to_string()]))
        );
        assert_eq!(
            db.search_keywords.get("query").unwrap(),
            &vec!["sql".to_string(), "select".to_string()]
        );
        assert_eq!(db.search_keywords.get("*").unwrap(), &vec!["database".to_string()]);

        let searcher = &parsed.servers["searcher"];
        assert_eq!(searcher.lifecycle, Some(McpLifecycleMode::LazyKeepAlive));
        assert_eq!(
            searcher.direct_tools,
            Some(McpDirectTools::Search("search".to_string()))
        );

        let all_direct = &parsed.servers["all_direct"];
        assert_eq!(all_direct.direct_tools, Some(McpDirectTools::All(true)));
    }
}
