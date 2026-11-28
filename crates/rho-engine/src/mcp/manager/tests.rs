use super::*;
use crate::mcp::cache::{McpCache, compute_definition_hash};
use crate::mcp::client::McpToolDefinition;
use crate::mcp::test_server::{self, Handler, Reply, eventually, handler, init_result, tools_result};
use rho_harness_core::config::ProjectMcpServers;
use serde_json::{Value, json};
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn always_unauthorized_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let reply = "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    url
}

fn remote(url: &str) -> McpServerConfig {
    McpServerConfig {
        url: Some(url.to_string()),
        enabled: true,
        ..Default::default()
    }
}

fn config_with(dir: &Path, servers: Vec<(&str, McpServerConfig)>) -> Config {
    let mut config = Config {
        config_dir: dir.join("config"),
        ..Default::default()
    };
    config.mcp.startup_timeout_ms = 0;
    for (name, cfg) in servers {
        config.mcp.servers.insert(name.to_string(), cfg);
    }
    config
}

fn standard(tools: &'static [&'static str], instructions: Option<&'static str>) -> Handler {
    handler(move |method, _| async move {
        match method.as_str() {
            "initialize" => init_result(instructions),
            "tools/list" => tools_result(tools),
            _ => Reply::Result(json!({})),
        }
    })
}

fn tool<'a>(load: &'a McpLoad, name: &str) -> &'a Arc<dyn EngineTool> {
    load.tools.iter().find(|t| t.name() == name).expect(name)
}

fn names(load: &McpLoad) -> Vec<&str> {
    load.tools.iter().map(|t| t.name()).collect()
}

fn tool_def(name: &str) -> McpToolDefinition {
    McpToolDefinition {
        name: name.to_string(),
        description: None,
        input_schema: json!({ "type": "object" }),
    }
}

#[tokio::test]
async fn connection_problems_are_reported_as_diagnostics() {
    let dir = crate::test_support::TestDir::new();
    let broken = McpServerConfig::stdio(dir.join("missing-mcp-binary").display().to_string(), Vec::new());
    let config = config_with(
        &dir,
        vec![
            ("remote", remote(&always_unauthorized_server().await)),
            ("broken", broken),
        ],
    );

    let load = load_mcp(&config, &dir, &HashSet::new()).await;

    assert!(load.tools.is_empty());
    assert_eq!(load.diagnostics.len(), 2, "{:?}", load.diagnostics);
    assert!(
        matches!(&load.diagnostics[0], McpDiagnostic::Failed { server, .. } if server == "broken"),
        "{:?}",
        load.diagnostics
    );
    assert_eq!(
        load.diagnostics[1],
        McpDiagnostic::NeedsLogin {
            server: "remote".into()
        }
    );
}

#[tokio::test]
async fn slow_servers_do_not_block_startup_and_join_the_catalog_later() {
    let dir = crate::test_support::TestDir::new();
    let gate = Arc::new(tokio::sync::Notify::new());
    let release = Arc::clone(&gate);
    let slow = test_server::spawn(handler(move |method, _| {
        let gate = Arc::clone(&gate);
        async move {
            match method.as_str() {
                "initialize" => init_result(None),
                "tools/list" => {
                    gate.notified().await;
                    tools_result(&["late_tool"])
                }
                _ => Reply::Result(json!({})),
            }
        }
    }))
    .await;
    let fast = test_server::spawn(standard(&["ping"], None)).await;
    let mut config = config_with(&dir, vec![("fast", remote(&fast.url)), ("slow", remote(&slow.url))]);
    config.mcp.startup_timeout_ms = 150;

    let load = load_mcp(&config, &dir, &HashSet::new()).await;

    assert_eq!(names(&load), ["fast_ping", "tool_search", "mcp"]);
    assert_eq!(load.diagnostics, [McpDiagnostic::Pending { server: "slow".into() }]);
    let catalog = load.runtime.as_ref().unwrap().catalog().clone();
    assert_eq!(catalog.server_states()["slow"], ServerState::Connecting);

    release.notify_one();
    eventually("late tools", || catalog.find(Some("slow"), "late_tool").is_some()).await;
    assert!(catalog.server_states().is_empty());
    assert_eq!(catalog.search("late", 5)[0].name, "late_tool");
}

fn stale_cache(config: &Config, server: &str, cfg: &McpServerConfig, updated_at: u64) {
    let mut cache = McpCache::default();
    cache.update(server, compute_definition_hash(cfg), vec![tool_def("old_tool")], None);
    cache.servers.get_mut(server).unwrap().updated_at = updated_at;
    cache.save_to_dir(&config.config_dir).unwrap();
}

#[tokio::test]
async fn fresh_cache_entries_start_without_contacting_the_server() {
    let dir = crate::test_support::TestDir::new();
    let server = test_server::spawn(standard(&["new_tool"], None)).await;
    let cfg = remote(&server.url);
    let config = config_with(&dir, vec![("srv", cfg.clone())]);
    let now = crate::clock::unix_now_secs();
    stale_cache(&config, "srv", &cfg, now);

    let load = load_mcp(&config, &dir, &HashSet::new()).await;
    tokio::task::yield_now().await;

    assert_eq!(names(&load), ["srv_old_tool", "mcp"]);
    assert!(server.methods().is_empty(), "{:?}", server.methods());
}

#[tokio::test]
async fn stale_cache_entries_are_used_then_refreshed_in_the_background() {
    let dir = crate::test_support::TestDir::new();
    let server = test_server::spawn(standard(&["new_tool"], None)).await;
    let cfg = remote(&server.url);
    let config = config_with(&dir, vec![("srv", cfg.clone())]);
    stale_cache(&config, "srv", &cfg, 0);

    let load = load_mcp(&config, &dir, &HashSet::new()).await;

    assert_eq!(names(&load), ["srv_old_tool", "mcp"]);
    let catalog = load.runtime.as_ref().unwrap().catalog().clone();
    eventually("refreshed catalog", || catalog.find(Some("srv"), "new_tool").is_some()).await;
    assert!(catalog.find(Some("srv"), "old_tool").is_none());
    let cache = McpCache::load_from_dir(&config.config_dir);
    assert_eq!(cache.servers["srv"].tools, [tool_def_described("new_tool")]);
}

fn tool_def_described(name: &str) -> McpToolDefinition {
    McpToolDefinition {
        description: Some(format!("{name} tool")),
        ..tool_def(name)
    }
}

#[tokio::test]
async fn server_instructions_are_collected_cached_and_can_be_disabled() {
    let dir = crate::test_support::TestDir::new();
    let docs = test_server::spawn(standard(&["a"], Some("Use project keys."))).await;
    let quiet = test_server::spawn(standard(&["b"], Some("Ignore me."))).await;
    let mut quiet_cfg = remote(&quiet.url);
    quiet_cfg.instructions = Some(false);
    let config = config_with(&dir, vec![("docs", remote(&docs.url)), ("quiet", quiet_cfg)]);
    let expected = [McpServerInstructions {
        server: "docs".into(),
        text: "Use project keys.".into(),
    }];

    let first = load_mcp(&config, &dir, &HashSet::new()).await;
    assert_eq!(first.instructions, expected);
    let requests = docs.methods().len();

    let cached = load_mcp(&config, &dir, &HashSet::new()).await;
    assert_eq!(cached.instructions, expected);
    assert_eq!(docs.methods().len(), requests, "second load must come from the cache");
}

fn gateway_server(tools: Arc<Mutex<Vec<&'static str>>>) -> Handler {
    handler(move |method, params| {
        let tools = Arc::clone(&tools);
        async move {
            match method.as_str() {
                "initialize" => init_result(None),
                "tools/list" => tools_result(&tools.lock().unwrap().clone()),
                "tools/call" => Reply::SseWith(
                    vec![json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" })],
                    json!({
                        "content": [{ "type": "text", "text": format!("called {}", params["name"]) }],
                        "structuredContent": { "echo": params["arguments"] }
                    }),
                ),
                "resources/list" => Reply::Result(json!({
                    "resources": [{ "uri": "memo://a", "name": "A", "description": "first memo" }]
                })),
                "resources/read" => Reply::Result(json!({
                    "contents": [{ "uri": params["uri"], "text": "memo body" }]
                })),
                _ => Reply::Result(Value::Null),
            }
        }
    })
}

async fn gateway(load: &McpLoad, args: Value) -> crate::tools::ToolResult {
    tool(load, "mcp").execute(args).await.unwrap()
}

#[tokio::test]
async fn gateway_calls_deferred_tools_and_reads_resources() {
    let dir = crate::test_support::TestDir::new();
    let server = test_server::spawn(gateway_server(Arc::new(Mutex::new(vec!["lookup"])))).await;
    let mut config = config_with(&dir, vec![("srv", remote(&server.url))]);
    config.mcp.defer_threshold = 0;
    let load = load_mcp(&config, &dir, &HashSet::new()).await;
    assert_eq!(names(&load), ["tool_search", "mcp"]);

    let called = gateway(
        &load,
        json!({ "server": "srv", "tool": "lookup", "arguments": { "q": 1 } }),
    )
    .await;
    assert!(!called.is_error, "{}", called.content);
    assert!(called.content.contains("called \"lookup\""), "{}", called.content);
    assert!(called.content.contains("\"echo\""), "{}", called.content);

    let listed = gateway(&load, json!({ "server": "srv", "action": "list_resources" })).await;
    assert_eq!(listed.content, "- memo://a (A): first memo");
    let read = gateway(
        &load,
        json!({ "server": "srv", "action": "read_resource", "uri": "memo://a" }),
    )
    .await;
    assert_eq!(read.content, "[Resource: memo://a]\nmemo body");

    let unknown = gateway(&load, json!({ "server": "srv", "tool": "nope" })).await;
    assert!(
        unknown.is_error && unknown.content.contains("tool_search"),
        "{}",
        unknown.content
    );
    let no_server = gateway(&load, json!({ "server": "other", "tool": "lookup" })).await;
    assert!(no_server.is_error && no_server.content.contains("Configured servers: srv"));
}

#[tokio::test]
async fn tools_list_changed_notifications_refresh_the_catalog() {
    let dir = crate::test_support::TestDir::new();
    let tools = Arc::new(Mutex::new(vec!["lookup"]));
    let server = test_server::spawn(gateway_server(Arc::clone(&tools))).await;
    let mut config = config_with(&dir, vec![("srv", remote(&server.url))]);
    config.mcp.defer_threshold = 0;
    let load = load_mcp(&config, &dir, &HashSet::new()).await;
    let catalog = load.runtime.as_ref().unwrap().catalog().clone();
    assert!(catalog.find(Some("srv"), "added").is_none());

    tools.lock().unwrap().push("added");
    let called = gateway(&load, json!({ "server": "srv", "tool": "lookup" })).await;
    assert!(!called.is_error, "{}", called.content);

    eventually("list_changed refresh", || catalog.find(Some("srv"), "added").is_some()).await;
    assert_eq!(
        names(&load),
        ["tool_search", "mcp"],
        "the provider tool list stays stable"
    );
}

#[tokio::test]
async fn direct_tool_names_that_collide_stay_behind_tool_search() {
    let dir = crate::test_support::TestDir::new();
    let server = test_server::spawn(standard(&["fetch"], None)).await;
    let config = config_with(&dir, vec![("web", remote(&server.url))]);

    let load = load_mcp(&config, &dir, &HashSet::new()).await;

    assert_eq!(names(&load), ["tool_search", "mcp"]);
    assert_eq!(
        load.diagnostics,
        [McpDiagnostic::NameConflict {
            server: "web".into(),
            tool: "fetch".into(),
            wire_name: "web_fetch".into(),
        }]
    );
    let entry = load
        .runtime
        .as_ref()
        .unwrap()
        .catalog()
        .find(Some("web"), "fetch")
        .unwrap();
    assert!(!entry.direct);
}

#[tokio::test]
async fn untrusted_project_servers_are_never_contacted() {
    let dir = crate::test_support::TestDir::new();
    std::fs::write(dir.join(".mcp.json"), "{}").unwrap();
    let server = test_server::spawn(standard(&["a"], None)).await;
    let mut config = config_with(&dir, vec![("proj", remote(&server.url))]);
    config.mcp.project = Some(project(&dir, &["proj"]));

    let load = load_mcp(&config, &dir, &HashSet::new()).await;

    assert!(load.runtime.is_none());
    assert_eq!(load.diagnostics, [McpDiagnostic::Untrusted { server: "proj".into() }]);
    assert!(server.methods().is_empty());
}

fn project(workspace: &Path, names: &[&str]) -> ProjectMcpServers {
    ProjectMcpServers {
        workspace: workspace.to_path_buf(),
        names: names.iter().map(|n| n.to_string()).collect(),
    }
}

#[tokio::test]
async fn global_servers_load_in_a_workspace_with_project_servers() {
    let dir = crate::test_support::TestDir::new();
    std::fs::write(dir.join(".mcp.json"), "{}").unwrap();
    let global = test_server::spawn(standard(&["g"], None)).await;
    let local = test_server::spawn(standard(&["p"], None)).await;
    let mut config = config_with(
        &dir,
        vec![("global", remote(&global.url)), ("proj", remote(&local.url))],
    );
    config.mcp.project = Some(project(&dir, &["proj"]));

    let load = load_mcp(&config, &dir, &HashSet::new()).await;

    assert_eq!(names(&load), ["global_g", "mcp"]);
    assert_eq!(load.diagnostics, [McpDiagnostic::Untrusted { server: "proj".into() }]);
    assert!(local.methods().is_empty());
}

#[tokio::test]
async fn approved_project_servers_load_until_their_definition_changes() {
    let dir = crate::test_support::TestDir::new();
    let server = test_server::spawn(standard(&["a"], None)).await;
    let mut config = config_with(&dir, vec![("proj", remote(&server.url))]);
    config.mcp.project = Some(project(&dir, &["proj"]));
    let approved = compute_definition_hash(&config.mcp.servers["proj"]);
    crate::mcp::approve_server_definition(&dir, "proj", &approved, &config.config_dir).unwrap();

    let load = load_mcp(&config, &dir, &HashSet::new()).await;
    assert_eq!(names(&load), ["proj_a", "mcp"]);
    assert!(load.diagnostics.is_empty(), "{:?}", load.diagnostics);

    config.mcp.servers.get_mut("proj").unwrap().url = Some(format!("{}?changed", server.url));
    let changed = load_mcp(&config, &dir, &HashSet::new()).await;
    assert_eq!(
        changed.diagnostics,
        [McpDiagnostic::Untrusted { server: "proj".into() }]
    );
}

fn crashing_server(crash_calls: usize) -> Handler {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    handler(move |method, _| {
        let calls = Arc::clone(&calls);
        async move {
            match method.as_str() {
                "initialize" => init_result(None),
                "tools/list" => tools_result(&["work"]),
                "tools/call" if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < crash_calls => Reply::Drop,
                "tools/call" => Reply::Result(json!({ "content": [{ "type": "text", "text": "worked" }] })),
                _ => Reply::Result(json!({})),
            }
        }
    })
}

async fn work(load: &McpLoad) -> crate::tools::ToolResult {
    tool(load, "srv_work").execute(json!({})).await.unwrap()
}

#[tokio::test]
async fn a_tool_call_lost_in_flight_is_not_resent_but_the_next_call_reconnects() {
    let dir = crate::test_support::TestDir::new();
    let server = test_server::spawn(crashing_server(1)).await;
    let config = config_with(&dir, vec![("srv", remote(&server.url))]);
    let load = load_mcp(&config, &dir, &HashSet::new()).await;

    let lost = work(&load).await;

    assert!(lost.is_error);
    assert!(lost.content.contains("not retried"), "{}", lost.content);
    let count = |server: &test_server::FakeServer| server.methods().iter().filter(|m| *m == "tools/call").count();
    assert_eq!(count(&server), 1, "a side-effecting call is never sent twice");

    let next = work(&load).await;

    assert!(!next.is_error, "{}", next.content);
    assert_eq!(next.content, "worked");
    assert_eq!(count(&server), 2);
    let methods = server.methods();
    let call_start = methods.iter().position(|m| m == "tools/list").unwrap() + 1;
    assert_eq!(
        methods[call_start..],
        [
            "initialize",
            "notifications/initialized",
            "tools/call",
            "initialize",
            "notifications/initialized",
            "tools/call"
        ]
    );
}

#[tokio::test]
async fn a_read_only_request_lost_in_flight_is_reconnected_and_resent() {
    let dir = crate::test_support::TestDir::new();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let server = test_server::spawn(handler(move |method, _| {
        let calls = Arc::clone(&calls);
        async move {
            match method.as_str() {
                "initialize" => init_result(None),
                "tools/list" => tools_result(&["work"]),
                "resources/list" if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 => Reply::Drop,
                _ => Reply::Result(json!({ "resources": [{ "uri": "file:///a", "name": "a" }] })),
            }
        }
    }))
    .await;
    let config = config_with(&dir, vec![("srv", remote(&server.url))]);
    let load = load_mcp(&config, &dir, &HashSet::new()).await;
    let runtime = Arc::clone(load.runtime.as_ref().unwrap());

    let result = runtime.list_resources("srv").await;

    assert!(!result.is_error, "{}", result.content);
    assert_eq!(server.methods().iter().filter(|m| *m == "resources/list").count(), 2);
}

#[tokio::test]
async fn servers_that_keep_dropping_stop_reconnecting_and_report_it() {
    let dir = crate::test_support::TestDir::new();
    let server = test_server::spawn(crashing_server(usize::MAX)).await;
    let config = config_with(&dir, vec![("srv", remote(&server.url))]);
    let load = load_mcp(&config, &dir, &HashSet::new()).await;
    let runtime = Arc::clone(load.runtime.as_ref().unwrap());

    for _ in 0..crate::mcp::runtime::MAX_RECONNECTS {
        let result = work(&load).await;
        assert!(result.content.contains("connection lost"), "{}", result.content);
    }
    let calls_before = server.methods().iter().filter(|m| *m == "tools/call").count();
    let suspended = work(&load).await;

    assert!(
        suspended.content.contains("reconnects suspended"),
        "{}",
        suspended.content
    );
    let calls_after = server.methods().iter().filter(|m| *m == "tools/call").count();
    assert_eq!(calls_after, calls_before + 1, "no retry once suspended");
    let problems = runtime.take_problems();
    assert!(
        matches!(&problems[..], [McpDiagnostic::Failed { server, message }] if server == "srv" && message.contains("suspended")),
        "{problems:?}"
    );
}

#[tokio::test]
async fn late_discovery_problems_are_reported_once_to_subscribers() {
    let dir = crate::test_support::TestDir::new();
    let gate = Arc::new(tokio::sync::Notify::new());
    let release = Arc::clone(&gate);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            gate.notified().await;
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let reply = "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    let mut config = config_with(&dir, vec![("late", remote(&url))]);
    config.mcp.startup_timeout_ms = 50;

    let load = load_mcp(&config, &dir, &HashSet::new()).await;
    assert_eq!(load.diagnostics, [McpDiagnostic::Pending { server: "late".into() }]);
    let runtime = Arc::clone(load.runtime.as_ref().unwrap());
    release.notify_one();

    tokio::time::timeout(Duration::from_secs(2), runtime.problems_ready())
        .await
        .unwrap();
    let problems = runtime.take_problems();
    assert_eq!(problems, [McpDiagnostic::NeedsLogin { server: "late".into() }]);
    runtime.adopt("late", Err(problems[0].clone()));
    assert!(
        runtime.take_problems().is_empty(),
        "duplicate problems are reported once"
    );
}
