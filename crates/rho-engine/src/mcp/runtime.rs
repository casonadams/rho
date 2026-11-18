use super::cache::{McpCache, compute_definition_hash};
use super::client::{McpClient, McpPromptDefinition, McpPromptResult, McpResourceDefinition, McpToolDefinition};
use super::manager::McpDiagnostic;
use super::pool::McpServerPool;
use super::result::{OutputLimits, resource_result, tool_result};
use super::search::{McpToolEntry, ServerState, ToolSearchCatalog};
use crate::tools::ToolResult;
use rho_harness_core::config::{Config, McpLifecycleMode, McpServerConfig};
use rho_harness_core::error::{AppError, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::Instant;

const TOOLS_LIST_CHANGED: &str = "notifications/tools/list_changed";
pub const CACHE_REFRESH_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
pub const MAX_RECONNECTS: usize = 5;
pub const RECONNECT_WINDOW: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerSnapshot {
    pub tools: Vec<McpToolDefinition>,
    pub instructions: Option<String>,
}

pub struct McpRuntime {
    pool: McpServerPool,
    catalog: ToolSearchCatalog,
    servers: BTreeMap<String, McpServerConfig>,
    config_dir: PathBuf,
    cache: tokio::sync::Mutex<McpCache>,
    max_bytes: usize,
    artifact_dir: PathBuf,
    problems: Mutex<Vec<McpDiagnostic>>,
    problems_ready: Notify,
    reported: Mutex<BTreeSet<McpDiagnostic>>,
    reconnects: Mutex<BTreeMap<String, Vec<Instant>>>,
}

struct InFlight(Arc<AtomicUsize>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn connect_diagnostic(name: &str, cfg: &McpServerConfig, err: &AppError) -> McpDiagnostic {
    if matches!(err, AppError::Auth(_)) && cfg.uses_oauth() {
        return McpDiagnostic::NeedsLogin {
            server: name.to_string(),
        };
    }
    McpDiagnostic::Failed {
        server: name.to_string(),
        message: err.to_string(),
    }
}

fn is_tool_allowed(name: &str, cfg: &McpServerConfig) -> bool {
    let matches = |patterns: &[String]| patterns.iter().any(|p| p == name || p == "*");
    if cfg.exclude_tools.as_deref().is_some_and(matches) {
        return false;
    }
    cfg.include_tools.as_deref().is_none_or(matches)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn format_resource_list(resources: &[McpResourceDefinition]) -> String {
    if resources.is_empty() {
        return "No resources.".to_string();
    }
    resources
        .iter()
        .map(|r| {
            let mut line = format!("- {} ({})", r.uri, r.name);
            if let Some(mime) = &r.mime_type {
                line.push_str(&format!(" [{mime}]"));
            }
            if let Some(desc) = &r.description {
                line.push_str(&format!(": {desc}"));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl McpRuntime {
    pub(crate) async fn start(
        config: &Config,
        working_dir: &Path,
        servers: BTreeMap<String, McpServerConfig>,
    ) -> Arc<Self> {
        let pool = McpServerPool::new(config.mcp.idle_timeout_seconds)
            .with_token_store(super::oauth::McpTokenStore::in_config_dir(&config.config_dir));
        pool.spawn_reaper_task(Duration::from_secs(30));
        for (name, cfg) in &servers {
            pool.register(name, cfg.clone(), working_dir).await;
        }
        let runtime = Arc::new(Self {
            pool,
            catalog: ToolSearchCatalog::default(),
            servers,
            config_dir: config.config_dir.clone(),
            cache: tokio::sync::Mutex::new(McpCache::load_from_dir(&config.config_dir)),
            max_bytes: config.output_max_bytes,
            artifact_dir: working_dir.join(".rho/artifacts"),
            problems: Mutex::new(Vec::new()),
            problems_ready: Notify::new(),
            reported: Mutex::new(BTreeSet::new()),
            reconnects: Mutex::new(BTreeMap::new()),
        });
        let weak = Arc::downgrade(&runtime);
        runtime
            .pool
            .set_notification_hook(Arc::new(move |server: &str, method: &str| {
                if method != TOOLS_LIST_CHANGED {
                    return;
                }
                if let Some(runtime) = weak.upgrade() {
                    let server = server.to_string();
                    tokio::spawn(async move { runtime.refresh(&server).await });
                }
            }));
        runtime
    }

    pub fn catalog(&self) -> &ToolSearchCatalog {
        &self.catalog
    }

    pub fn server_names(&self) -> impl Iterator<Item = &str> {
        self.servers.keys().map(String::as_str)
    }

    pub fn server_config(&self, server: &str) -> Option<&McpServerConfig> {
        self.servers.get(server)
    }

    pub(crate) fn pool(&self) -> &McpServerPool {
        &self.pool
    }

    pub fn take_problems(&self) -> Vec<McpDiagnostic> {
        std::mem::take(&mut *self.problems.lock().unwrap())
    }

    pub async fn problems_ready(&self) {
        self.problems_ready.notified().await;
    }

    fn report(&self, diagnostic: McpDiagnostic) {
        if self.reported.lock().unwrap().insert(diagnostic.clone()) {
            self.problems.lock().unwrap().push(diagnostic);
            self.problems_ready.notify_one();
        }
    }

    fn report_error(&self, server: &str, err: &AppError) {
        if let Some(cfg) = self.servers.get(server)
            && matches!(err, AppError::Auth(_))
        {
            self.report(connect_diagnostic(server, cfg, err));
        }
    }

    fn allow_reconnect(&self, server: &str) -> bool {
        let now = Instant::now();
        let mut reconnects = self.reconnects.lock().unwrap();
        let recent = reconnects.entry(server.to_string()).or_default();
        recent.retain(|at| now.duration_since(*at) < RECONNECT_WINDOW);
        if recent.len() >= MAX_RECONNECTS {
            return false;
        }
        recent.push(now);
        true
    }

    fn limits(&self) -> OutputLimits<'_> {
        OutputLimits {
            max_bytes: self.max_bytes,
            artifact_dir: Some(&self.artifact_dir),
        }
    }

    async fn attempt<T, F, Fut>(&self, server: &str, op: &F) -> Result<T>
    where
        F: Fn(Arc<McpClient>) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let (client, in_flight) = self.pool.get_or_connect(server).await?;
        let in_flight = InFlight(in_flight);
        let res = op(client).await;
        drop(in_flight);
        self.pool.touch_activity(server).await;
        res
    }

    async fn with_client<T, F, Fut>(&self, server: &str, op: F) -> Result<T>
    where
        F: Fn(Arc<McpClient>) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let res = match self.attempt(server, &op).await {
            Err(AppError::McpConnectionLost(reason)) => {
                self.pool.disconnect(server).await;
                if !self.allow_reconnect(server) {
                    let message = format!(
                        "connection lost {MAX_RECONNECTS} times within {}s; reconnects suspended until /reload ({reason})",
                        RECONNECT_WINDOW.as_secs()
                    );
                    self.report(McpDiagnostic::Failed {
                        server: server.to_string(),
                        message: message.clone(),
                    });
                    return Err(AppError::Mcp(message));
                }
                self.attempt(server, &op).await
            }
            other => other,
        };
        if let Err(e) = &res {
            self.report_error(server, e);
        }
        res
    }

    pub async fn call_tool(&self, server: &str, tool: &str, arguments: Value) -> ToolResult {
        let res = self
            .with_client(server, |client| {
                let arguments = arguments.clone();
                async move { client.call_tool(tool, arguments).await }
            })
            .await;
        match res {
            Ok(result) => tool_result(&result, self.limits()),
            Err(e) => ToolResult::error(format!("[MCP Error] server '{server}': {e}")),
        }
    }

    pub async fn list_resources(&self, server: &str) -> ToolResult {
        match self
            .with_client(server, |client| async move { client.list_resources().await })
            .await
        {
            Ok(resources) => ToolResult::success(format_resource_list(&resources)),
            Err(e) => ToolResult::error(format!("[MCP Error] server '{server}': {e}")),
        }
    }

    pub async fn read_resource(&self, server: &str, uri: &str) -> ToolResult {
        match self
            .with_client(server, |client| async move { client.read_resource(uri).await })
            .await
        {
            Ok(contents) => resource_result(&contents, self.limits()),
            Err(e) => ToolResult::error(format!("[MCP Error] server '{server}': {e}")),
        }
    }

    pub async fn list_prompts(&self, server: &str) -> Result<Vec<McpPromptDefinition>> {
        self.with_client(server, |client| async move { client.list_prompts().await })
            .await
    }

    pub async fn get_prompt(
        &self,
        server: &str,
        name: &str,
        arguments: &BTreeMap<String, String>,
    ) -> Result<McpPromptResult> {
        self.with_client(server, |client| async move { client.get_prompt(name, arguments).await })
            .await
    }

    pub(crate) async fn cached_snapshot(&self, server: &str) -> Option<(ServerSnapshot, bool)> {
        let cfg = self.servers.get(server)?;
        let cache = self.cache.lock().await;
        let entry = cache.get_valid(server, &compute_definition_hash(cfg))?;
        let fresh = entry.updated_at + CACHE_REFRESH_AFTER.as_secs() > unix_now();
        let snapshot = ServerSnapshot {
            tools: entry.tools.clone(),
            instructions: entry.instructions.clone(),
        };
        Some((snapshot, fresh))
    }

    pub(crate) async fn fetch_snapshot(&self, server: &str) -> std::result::Result<ServerSnapshot, McpDiagnostic> {
        let Some(cfg) = self.servers.get(server) else {
            return Err(McpDiagnostic::Failed {
                server: server.to_string(),
                message: "server is not configured".to_string(),
            });
        };
        let (client, in_flight) = self
            .pool
            .get_or_connect(server)
            .await
            .map_err(|e| connect_diagnostic(server, cfg, &e))?;
        let in_flight = InFlight(in_flight);
        let listed = client.list_tools().await;
        drop(in_flight);
        if cfg.lifecycle.unwrap_or(McpLifecycleMode::Lazy) == McpLifecycleMode::Lazy {
            self.pool.disconnect(server).await;
        }
        let tools = listed.map_err(|e| McpDiagnostic::Failed {
            server: server.to_string(),
            message: format!("listing tools failed: {e}"),
        })?;
        let snapshot = ServerSnapshot {
            tools,
            instructions: client.instructions().map(str::to_string),
        };
        let mut cache = self.cache.lock().await;
        cache.update(
            server,
            compute_definition_hash(cfg),
            snapshot.tools.clone(),
            snapshot.instructions.clone(),
        );
        let _ = cache.save_to_dir(&self.config_dir);
        Ok(snapshot)
    }

    pub(crate) fn entries(&self, server: &str, tools: Vec<McpToolDefinition>) -> Vec<McpToolEntry> {
        let Some(cfg) = self.servers.get(server) else {
            return Vec::new();
        };
        tools
            .into_iter()
            .filter(|t| is_tool_allowed(&t.name, cfg))
            .map(|tool| McpToolEntry::new(server, tool, &cfg.search_keywords))
            .collect()
    }

    pub(crate) fn adopt(&self, server: &str, result: std::result::Result<ServerSnapshot, McpDiagnostic>) {
        match result {
            Ok(snapshot) => {
                let entries = self.entries(server, snapshot.tools);
                self.catalog.replace_server_tools(server, entries);
            }
            Err(diagnostic) => {
                self.catalog
                    .set_server_state(server, ServerState::Unavailable(diagnostic.message()));
                self.report(diagnostic);
            }
        }
    }

    pub(crate) async fn refresh(&self, server: &str) {
        match self.fetch_snapshot(server).await {
            Ok(snapshot) => {
                let entries = self.entries(server, snapshot.tools);
                self.catalog.replace_server_tools(server, entries);
            }
            Err(diagnostic) => {
                self.catalog
                    .set_server_state(server, ServerState::Stale(diagnostic.message()));
                self.report(diagnostic);
            }
        }
    }
}
