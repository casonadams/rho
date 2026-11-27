use super::gateway::{GATEWAY_TOOL_NAME, build_gateway_tool};
use super::instructions::McpServerInstructions;
use super::naming::NameRegistry;
use super::runtime::{McpRuntime, ServerSnapshot};
use super::search::{McpToolEntry, ServerState, build_tool_search_tool};
use super::trust::project_server_approval;
use crate::tools::{EngineTool, GenericTool};
use rho_harness_core::config::{Config, McpDirectTools, McpExposureMode, McpLifecycleMode, McpServerConfig};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum McpDiagnostic {
    NeedsLogin {
        server: String,
    },
    Untrusted {
        server: String,
    },
    Failed {
        server: String,
        message: String,
    },
    Pending {
        server: String,
    },
    NameConflict {
        server: String,
        tool: String,
        wire_name: String,
    },
}

impl McpDiagnostic {
    pub fn server(&self) -> &str {
        match self {
            Self::NeedsLogin { server }
            | Self::Untrusted { server }
            | Self::Failed { server, .. }
            | Self::Pending { server }
            | Self::NameConflict { server, .. } => server,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::NeedsLogin { server } => format!("sign-in required; run /mcp login {server}"),
            Self::Untrusted { server } => {
                format!("project definition has not been approved; run rho mcp approve {server}")
            }
            Self::Failed { message, .. } => message.clone(),
            Self::Pending { .. } => "still connecting".to_string(),
            Self::NameConflict { tool, wire_name, .. } => {
                format!("tool '{tool}' kept behind tool_search: name '{wire_name}' is already taken")
            }
        }
    }
}

pub struct McpLoad {
    pub tools: Vec<Arc<dyn EngineTool>>,
    pub diagnostics: Vec<McpDiagnostic>,
    pub instructions: Vec<McpServerInstructions>,
    pub runtime: Option<Arc<McpRuntime>>,
}

impl McpLoad {
    fn empty() -> Self {
        Self {
            tools: Vec::new(),
            diagnostics: Vec::new(),
            instructions: Vec::new(),
            runtime: None,
        }
    }
}

fn trusted_servers(config: &Config, diagnostics: &mut Vec<McpDiagnostic>) -> BTreeMap<String, McpServerConfig> {
    let mut trusted = BTreeMap::new();
    for (name, cfg) in config.mcp.servers.iter().filter(|(_, cfg)| cfg.enabled) {
        if project_server_approval(config, name, cfg) == Some(false) {
            diagnostics.push(McpDiagnostic::Untrusted { server: name.clone() });
        } else {
            trusted.insert(name.clone(), cfg.clone());
        }
    }
    trusted
}

#[derive(Default)]
struct StartupPlan {
    ready: BTreeMap<String, ServerSnapshot>,
    stale: Vec<String>,
    discover: Vec<String>,
}

async fn plan_startup(runtime: &McpRuntime) -> StartupPlan {
    let mut plan = StartupPlan::default();
    let names: Vec<String> = runtime.server_names().map(str::to_string).collect();
    for name in names {
        match runtime.cached_snapshot(&name).await {
            Some((snapshot, fresh)) => {
                if !fresh {
                    plan.stale.push(name.clone());
                }
                plan.ready.insert(name, snapshot);
            }
            None => plan.discover.push(name),
        }
    }
    plan
}

type Discovery = (String, Result<ServerSnapshot, McpDiagnostic>);

async fn discover_within(
    runtime: &Arc<McpRuntime>,
    servers: Vec<String>,
    wait: Option<Duration>,
) -> (Vec<Discovery>, Vec<String>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Discovery>();
    for name in &servers {
        let (runtime, tx, name) = (Arc::clone(runtime), tx.clone(), name.clone());
        tokio::spawn(async move {
            let result = runtime.fetch_snapshot(&name).await;
            let _ = tx.send((name, result));
        });
    }
    drop(tx);
    let deadline = wait.map(|w| tokio::time::Instant::now() + w);
    let mut finished = Vec::new();
    while finished.len() < servers.len() {
        let next = match deadline {
            Some(deadline) => tokio::time::timeout_at(deadline, rx.recv()).await.ok().flatten(),
            None => rx.recv().await,
        };
        match next {
            Some(item) => finished.push(item),
            None => break,
        }
    }
    let done: HashSet<&str> = finished.iter().map(|(name, _)| name.as_str()).collect();
    let pending: Vec<String> = servers.iter().filter(|s| !done.contains(s.as_str())).cloned().collect();
    if !pending.is_empty() {
        let runtime = Arc::clone(runtime);
        tokio::spawn(async move {
            while let Some((name, result)) = rx.recv().await {
                runtime.adopt(&name, result);
            }
        });
    }
    (finished, pending)
}

fn wants_direct(entry: &McpToolEntry, cfg: &McpServerConfig, should_defer: bool, history: &HashSet<String>) -> bool {
    if history.contains(&entry.wire_name) {
        return true;
    }
    match &cfg.direct_tools {
        Some(McpDirectTools::All(all)) => *all,
        Some(McpDirectTools::Search(_)) => false,
        Some(McpDirectTools::List(list)) => list.iter().any(|t| *t == entry.name || *t == entry.wire_name),
        None => cfg.mode == Some(McpExposureMode::Direct) || !should_defer,
    }
}

fn direct_tool(runtime: &Arc<McpRuntime>, entry: &McpToolEntry) -> Arc<dyn EngineTool> {
    let mut schema = entry.input_schema.clone();
    crate::tools::normalize_schema(&mut schema);
    let description = format!("[MCP: {}] {}", entry.server_name, entry.description);
    let (runtime, server, tool) = (Arc::clone(runtime), entry.server_name.clone(), entry.name.clone());
    Arc::new(GenericTool::new(
        entry.wire_name.clone(),
        description,
        schema,
        move |args| {
            let (runtime, server, tool) = (Arc::clone(&runtime), server.clone(), tool.clone());
            async move { Ok(runtime.call_tool(&server, &tool, args).await) }
        },
    ))
}

struct Routing<'a> {
    runtime: &'a Arc<McpRuntime>,
    should_defer: bool,
    history: &'a HashSet<String>,
    names: NameRegistry,
    tools: Vec<Arc<dyn EngineTool>>,
    diagnostics: Vec<McpDiagnostic>,
}

impl Routing<'_> {
    fn route(&mut self, server: &str, cfg: &McpServerConfig, mut entries: Vec<McpToolEntry>) {
        for entry in &mut entries {
            if !wants_direct(entry, cfg, self.should_defer, self.history) {
                continue;
            }
            match self.names.claim(server, &entry.name) {
                Ok(_) => {
                    entry.direct = true;
                    self.tools.push(direct_tool(self.runtime, entry));
                }
                Err(conflict) => self.diagnostics.push(McpDiagnostic::NameConflict {
                    server: conflict.server,
                    tool: conflict.tool,
                    wire_name: conflict.wire_name,
                }),
            }
        }
        self.runtime.catalog().replace_server_tools(server, entries);
    }
}

fn reserved_names() -> Vec<String> {
    let mut names = crate::engine::context::default_tool_names();
    names.extend(["tool_search".to_string(), GATEWAY_TOOL_NAME.to_string()]);
    names
}

fn server_instructions(runtime: &McpRuntime, ready: &BTreeMap<String, ServerSnapshot>) -> Vec<McpServerInstructions> {
    ready
        .iter()
        .filter(|(name, _)| {
            runtime
                .server_config(name)
                .is_some_and(|c| c.instructions != Some(false))
        })
        .filter_map(|(name, snapshot)| {
            snapshot.instructions.as_ref().map(|text| McpServerInstructions {
                server: name.clone(),
                text: text.clone(),
            })
        })
        .collect()
}

fn spawn_background_work(runtime: &Arc<McpRuntime>, stale: Vec<String>) {
    for name in stale {
        let runtime = Arc::clone(runtime);
        tokio::spawn(async move { runtime.refresh(&name).await });
    }
    let eager: Vec<String> = runtime
        .server_names()
        .filter(|name| {
            runtime
                .server_config(name)
                .is_some_and(|c| c.lifecycle == Some(McpLifecycleMode::Eager))
        })
        .map(str::to_string)
        .collect();
    for name in eager {
        let runtime = Arc::clone(runtime);
        tokio::spawn(async move {
            if let Err(err) = runtime.pool().get_or_connect(&name).await {
                tracing::warn!(server = %name, error = %err, "eager MCP connection failed");
            }
        });
    }
}

fn startup_wait(config: &Config) -> Option<Duration> {
    (config.mcp.startup_timeout_ms > 0).then(|| Duration::from_millis(config.mcp.startup_timeout_ms))
}

pub async fn load_mcp_tools(config: &Config, working_dir: &Path) -> Vec<Arc<dyn EngineTool>> {
    load_mcp(config, working_dir, &HashSet::new()).await.tools
}

pub async fn load_mcp(config: &Config, working_dir: &Path, active_history_tools: &HashSet<String>) -> McpLoad {
    if !config.mcp.enabled {
        return McpLoad::empty();
    }
    let mut diagnostics = Vec::new();
    let servers = trusted_servers(config, &mut diagnostics);
    if servers.is_empty() {
        return McpLoad {
            diagnostics,
            ..McpLoad::empty()
        };
    }
    let runtime = McpRuntime::start(config, working_dir, servers).await;
    let mut plan = plan_startup(&runtime).await;
    let (mut finished, pending) =
        discover_within(&runtime, std::mem::take(&mut plan.discover), startup_wait(config)).await;
    finished.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, result) in finished {
        match result {
            Ok(snapshot) => {
                plan.ready.insert(name, snapshot);
            }
            Err(diagnostic) => {
                runtime
                    .catalog()
                    .set_server_state(&name, ServerState::Unavailable(diagnostic.message()));
                diagnostics.push(diagnostic);
            }
        }
    }
    for name in &pending {
        runtime.catalog().set_server_state(name, ServerState::Connecting);
        diagnostics.push(McpDiagnostic::Pending { server: name.clone() });
    }

    let entries: Vec<(String, Vec<McpToolEntry>)> = plan
        .ready
        .iter()
        .map(|(name, snapshot)| (name.clone(), runtime.entries(name, snapshot.tools.clone())))
        .collect();
    let total_tools: usize = entries.iter().map(|(_, e)| e.len()).sum();
    let mut routing = Routing {
        runtime: &runtime,
        should_defer: total_tools > config.mcp.defer_threshold,
        history: active_history_tools,
        names: NameRegistry::with_reserved(reserved_names()),
        tools: Vec::new(),
        diagnostics: Vec::new(),
    };
    for (name, server_entries) in entries {
        if let Some(cfg) = runtime.server_config(&name).cloned() {
            routing.route(&name, &cfg, server_entries);
        }
    }
    let mut tools = routing.tools;
    diagnostics.extend(routing.diagnostics);

    let catalog = runtime.catalog();
    if catalog.deferred_count() > 0 || !pending.is_empty() {
        tools.push(build_tool_search_tool(catalog.clone()));
    }
    if !catalog.is_empty() || !pending.is_empty() {
        tools.push(build_gateway_tool(Arc::clone(&runtime)));
    }
    spawn_background_work(&runtime, plan.stale);
    McpLoad {
        tools,
        diagnostics,
        instructions: server_instructions(&runtime, &plan.ready),
        runtime: Some(runtime),
    }
}

#[cfg(test)]
mod tests;
