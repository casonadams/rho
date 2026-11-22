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
    assert!(!remote.uses_oauth());
    assert!(!gh.uses_oauth());
}

#[test]
fn parses_oauth_block_in_camel_or_snake_case() {
    let json = r#"{
        "a": { "url": "https://a.example/mcp", "oauth": { "clientId": "cid", "clientSecret": "sec", "scopes": ["read", "write"], "redirectPort": 8765 } },
        "b": { "url": "https://b.example/mcp", "oauth": { "client_id": "other", "redirect_port": 99999 } },
        "c": { "url": "https://c.example/mcp" }
    }"#;
    let parsed = parse_mcp_json_str(json).unwrap();
    assert_eq!(
        parsed.servers["a"].oauth,
        Some(McpOAuthConfig {
            client_id: Some("cid".into()),
            client_secret: Some("sec".into()),
            scopes: vec!["read".into(), "write".into()],
            redirect_port: Some(8765),
            ..Default::default()
        })
    );
    let b = parsed.servers["b"].oauth.as_ref().unwrap();
    assert_eq!(b.client_id.as_deref(), Some("other"));
    assert_eq!(b.redirect_port, None);
    assert_eq!(parsed.servers["c"].oauth, None);
    assert!(parsed.servers["c"].uses_oauth());
}

#[test]
fn configured_headers_mean_static_credentials_unless_oauth_is_explicit() {
    let json = r#"{
        "api_key": { "url": "https://m.example/mcp", "headers": { "x-api-key": "k" } },
        "bearer": { "url": "https://b.example/mcp", "headers": { "Authorization": "Bearer t" } },
        "both": { "url": "https://o.example/mcp", "headers": { "X-Client": "rho" }, "oauth": {} },
        "bare": { "url": "https://c.example/mcp" },
        "local": { "command": "echo" }
    }"#;
    let parsed = parse_mcp_json_str(json).unwrap();
    let uses = |name: &str| parsed.servers[name].uses_oauth();
    assert!(!uses("api_key"));
    assert!(!uses("bearer"));
    assert!(uses("both"));
    assert!(uses("bare"));
    assert!(!uses("local"));
}

#[test]
fn parses_oauth_redirect_uri_scope_string_and_authorization_params() {
    let json = r#"{
        "g": { "url": "https://g.example/mcp", "oauth": {
            "clientId": "cid",
            "redirectUri": "http://localhost:19876/callback",
            "scope": "a  b",
            "authorizationParams": { "access_type": "offline", "prompt": "consent" }
        } },
        "s": { "url": "https://s.example/mcp", "oauth": { "scopes": "x y" }, "instructions": false }
    }"#;
    let parsed = parse_mcp_json_str(json).unwrap();
    let g = parsed.servers["g"].oauth.as_ref().unwrap();
    assert_eq!(g.redirect_uri.as_deref(), Some("http://localhost:19876/callback"));
    assert_eq!(g.scopes, ["a", "b"]);
    assert_eq!(g.authorization_params["access_type"], "offline");
    assert_eq!(g.authorization_params["prompt"], "consent");
    assert_eq!(parsed.servers["s"].oauth.as_ref().unwrap().scopes, ["x", "y"]);
    assert_eq!(parsed.servers["s"].instructions, Some(false));
    assert_eq!(parsed.servers["g"].instructions, None);
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
    let mcp_path = temp_mcp_file(None);

    let server = McpServerConfig::stdio("echo", vec!["hello".to_string()]);
    write_mcp_server_json(&mcp_path, "echo_srv", &server).unwrap();

    let parsed = parse_mcp_json_str(&std::fs::read_to_string(&mcp_path).unwrap()).unwrap();
    assert_eq!(parsed.servers["echo_srv"].command.as_deref(), Some("echo"));

    remove_mcp_server_json(&mcp_path, "echo_srv").unwrap();

    let after_removal = parse_mcp_json_str(&std::fs::read_to_string(&mcp_path).unwrap()).unwrap();
    assert!(!after_removal.servers.contains_key("echo_srv"));
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
    (
        "remove_drops_a_malformed_entry",
        r#"{"numStartups": 3, "mcpServers": {"a": 3, "b": {"command": "y"}}}"#,
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
        "mcpServers": {
            "db": {
                "command": "node",
                "args": ["server.js"],
                "lifecycle": "keep-alive",
                "idleTimeout": 120,
                "timeout_seconds": 30,
                "includeTools": ["query"],
                "exclude_tools": [],
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
                "lifecycle": "lazy-keep-alive",
                "idle_timeout_seconds": 60
            },
            "all_direct": {
                "command": "echo",
                "directTools": true
            }
        }
    }"#;

    let parsed = parse_mcp_json_str(json).unwrap();

    let db = &parsed.servers["db"];
    assert_eq!(db.lifecycle, Some(McpLifecycleMode::KeepAlive));
    assert_eq!(db.idle_timeout_seconds, Some(120));
    assert_eq!(db.timeout_seconds, Some(30));
    assert_eq!(db.include_tools, Some(vec!["query".to_string()]));
    assert_eq!(db.exclude_tools, None);
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
    assert_eq!(searcher.idle_timeout_seconds, Some(60));
    assert_eq!(
        searcher.direct_tools,
        Some(McpDirectTools::Search("search".to_string()))
    );

    let all_direct = &parsed.servers["all_direct"];
    assert_eq!(all_direct.direct_tools, Some(McpDirectTools::All(true)));
}

const SERVER_EDIT_FAILURES: &[(&str, Option<&str>, ServerEdit, &str)] = &[
    (
        "non_object_root",
        Some("[1]"),
        ServerEdit::Write("new"),
        "must contain a JSON object",
    ),
    ("malformed_json", Some("{"), ServerEdit::Write("new"), "Failed to parse"),
    (
        "servers_not_object",
        Some(r#"{"mcpServers": 3}"#),
        ServerEdit::Write("new"),
        "must be a JSON object",
    ),
    ("remove_missing_file", None, ServerEdit::Remove("a"), "does not exist"),
    (
        "remove_unknown_server",
        Some("{}"),
        ServerEdit::Remove("a"),
        "is not configured",
    ),
];

#[test]
fn server_edits_reject_invalid_files() {
    for (name, initial, edit, expected) in SERVER_EDIT_FAILURES {
        let path = temp_mcp_file(*initial);
        let error = match edit {
            ServerEdit::Write(server) => {
                write_mcp_server_json(&path, server, &McpServerConfig::stdio("echo", vec![])).unwrap_err()
            }
            ServerEdit::Remove(server) => remove_mcp_server_json(&path, server).unwrap_err(),
        }
        .to_string();
        assert!(error.contains(expected), "{name}: {error}");
    }
}

#[test]
fn server_write_reports_unreadable_config_path() {
    let dir = crate::test_support::created_temp_dir();
    let error = write_mcp_server_json(&dir, "new", &McpServerConfig::stdio("echo", vec![]))
        .unwrap_err()
        .to_string();
    assert!(error.contains("Failed to read"), "{error}");
}
