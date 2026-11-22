//! Starts language servers on demand and routes each file to the servers that handle it.

use super::client::{ClientSpec, LspClient};
use super::config::{ServerConfig, layer_paths, read_layer, resolve_command, resolve_servers};
pub use super::process::{Launched, Launcher, ProcessLauncher};
use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

type StartFuture = Shared<BoxFuture<'static, Result<Arc<LspClient>, String>>>;

/// A server that handles a file, with the root it runs in.
pub struct Candidate<'a> {
    pub server: &'a ServerConfig,
    pub command: PathBuf,
    pub root: PathBuf,
}

pub struct LspManager {
    workspace: PathBuf,
    servers: Vec<ServerConfig>,
    config_error: Option<String>,
    path_var: Option<OsString>,
    launcher: Arc<dyn Launcher>,
    slots: Mutex<HashMap<(String, PathBuf), StartFuture>>,
    reported_failures: Mutex<std::collections::HashSet<String>>,
}

static MANAGERS: LazyLock<Mutex<HashMap<PathBuf, Arc<LspManager>>>> = LazyLock::new(Default::default);

impl LspManager {
    pub fn new(
        workspace: &Path,
        servers: Result<Vec<ServerConfig>, String>,
        path_var: Option<OsString>,
        launcher: Arc<dyn Launcher>,
    ) -> Self {
        let (servers, config_error) = match servers {
            Ok(servers) => (servers, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        Self {
            workspace: workspace.canonicalize().unwrap_or_else(|_| workspace.to_path_buf()),
            servers,
            config_error,
            path_var,
            launcher,
            slots: Mutex::new(HashMap::new()),
            reported_failures: Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// The process-wide manager for `workspace`, so engine rebuilds keep their running servers.
    pub fn for_workspace(workspace: &Path) -> Arc<Self> {
        let mut managers = MANAGERS.lock().unwrap_or_else(PoisonError::into_inner);
        let manager = managers.entry(workspace.to_path_buf()).or_insert_with(|| {
            let home = rho_harness_core::config::home_dir();
            let layers: Result<Vec<_>, String> = layer_paths(home.as_deref(), workspace)
                .iter()
                .map(|path| read_layer(path))
                .collect();
            Arc::new(Self::new(
                workspace,
                layers.and_then(resolve_servers),
                std::env::var_os("PATH"),
                Arc::new(ProcessLauncher),
            ))
        });
        Arc::clone(manager)
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn servers(&self) -> &[ServerConfig] {
        &self.servers
    }

    pub fn is_installed(&self, server: &ServerConfig) -> bool {
        resolve_command(&server.command, self.path_var.as_deref()).is_some()
    }

    fn candidate<'a>(&self, server: &'a ServerConfig, path: &Path) -> Option<Candidate<'a>> {
        let command = resolve_command(&server.command, self.path_var.as_deref())?;
        let root = server.root_for(path, &self.workspace)?;
        Some(Candidate { server, command, root })
    }

    /// Servers that can run for `path`: the main one first, then linters.
    pub fn candidates(&self, path: &Path) -> Vec<Candidate<'_>> {
        let path = canonical(path);
        let path = path.as_path();
        let mut found = Vec::new();
        let mut has_main = false;
        for server in self.servers.iter().filter(|s| s.handles(path)) {
            if (server.linter || !has_main)
                && let Some(candidate) = self.candidate(server, path)
            {
                has_main |= !server.linter;
                found.push(candidate);
            }
        }
        found
    }

    /// Why no server can answer for `path`.
    pub fn unavailable_reason(&self, path: &Path) -> String {
        if let Some(error) = &self.config_error {
            return format!("lsp.json is invalid: {error}");
        }
        if let Some(failure) = self.start_failure(path) {
            return failure;
        }
        let handlers: Vec<&ServerConfig> = self.servers.iter().filter(|s| s.handles(path)).collect();
        if handlers.is_empty() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            return format!("no language server is configured for '.{ext}' files; add one in .agents/lsp.json");
        }
        let reasons: Vec<String> = handlers
            .iter()
            .map(|server| {
                if resolve_command(&server.command, self.path_var.as_deref()).is_none() {
                    format!("{}: `{}` is not on PATH", server.name, server.command)
                } else {
                    format!(
                        "{}: no root marker ({}) above the file",
                        server.name,
                        server.root_markers.join(", ")
                    )
                }
            })
            .collect();
        format!("no language server available: {}", reasons.join("; "))
    }

    fn start_failure(&self, path: &Path) -> Option<String> {
        let slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        self.candidates(path).iter().find_map(|candidate| {
            let key = (candidate.server.name.clone(), candidate.root.clone());
            match slots.get(&key)?.peek() {
                Some(Err(error)) => Some(error.clone()),
                _ => None,
            }
        })
    }

    fn slot(&self, candidate: &Candidate<'_>) -> StartFuture {
        let key = (candidate.server.name.clone(), candidate.root.clone());
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(existing) = slots.get(&key)
            && !matches!(existing.peek(), Some(Ok(client)) if !client.is_alive())
        {
            return existing.clone();
        }
        let start = start_client(
            Arc::clone(&self.launcher),
            candidate.server.clone(),
            candidate.command.clone(),
            candidate.root.clone(),
        )
        .boxed()
        .shared();
        tokio::spawn(start.clone().map(|_| ()));
        slots.insert(key, start.clone());
        start
    }

    /// Starts (or reuses) every server for `path` without waiting for them.
    pub fn warm(&self, path: &Path) {
        for candidate in self.candidates(path) {
            drop(self.slot(&candidate));
        }
    }

    /// The main server for `path`, started if needed.
    pub async fn client_for(&self, path: &Path) -> Result<Arc<LspClient>, String> {
        let start = {
            let candidates = self.candidates(path);
            let main = candidates.iter().find(|c| !c.server.linter);
            match main {
                Some(candidate) => self.slot(candidate),
                None => return Err(self.unavailable_reason(path)),
            }
        };
        start.await
    }

    /// Every server for `path`, each given at most `wait` to finish starting. A server that failed
    /// to start is reported once, then left out.
    pub async fn clients_within(&self, path: &Path, wait: std::time::Duration) -> Vec<Result<Arc<LspClient>, String>> {
        let starts: Vec<(String, StartFuture)> = self
            .candidates(path)
            .iter()
            .map(|c| (format!("{}:{}", c.server.name, c.root.display()), self.slot(c)))
            .collect();
        let waits = starts.into_iter().map(|(key, start)| async move {
            let name = key.split(':').next().unwrap_or_default().to_string();
            match tokio::time::timeout(wait, start).await {
                Ok(Err(error)) => (Some(key), Err(error)),
                Ok(Ok(client)) => (None, Ok(client)),
                Err(_) => (None, Err(format!("{name} is still starting"))),
            }
        });
        let results = futures::future::join_all(waits).await;
        let mut reported = self.reported_failures.lock().unwrap_or_else(PoisonError::into_inner);
        results
            .into_iter()
            .filter(|(failed, _)| failed.as_ref().is_none_or(|key| reported.insert(key.clone())))
            .map(|(_, result)| result)
            .collect()
    }

    /// Clients that finished starting and are still running.
    pub fn running_clients(&self) -> Vec<Arc<LspClient>> {
        let slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        slots
            .values()
            .filter_map(|start| match start.peek() {
                Some(Ok(client)) if client.is_alive() => Some(Arc::clone(client)),
                _ => None,
            })
            .collect()
    }

    /// The running clients, or when none run, the servers whose root marker is in the workspace root.
    pub async fn workspace_clients(&self) -> Vec<Arc<LspClient>> {
        let running = self.running_clients();
        if !running.is_empty() {
            return running;
        }
        let starts: Vec<StartFuture> = self
            .servers
            .iter()
            .filter(|server| !server.linter && !server.root_markers.is_empty())
            .filter_map(|server| {
                let command = resolve_command(&server.command, self.path_var.as_deref())?;
                let root = server.root_for(&self.workspace.join("_"), &self.workspace)?;
                (root == self.workspace).then(|| self.slot(&Candidate { server, command, root }))
            })
            .collect();
        futures::future::join_all(starts)
            .await
            .into_iter()
            .filter_map(Result::ok)
            .collect()
    }

    /// Running and failed servers, one line each.
    pub fn status_lines(&self) -> Vec<String> {
        let slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        let mut lines: Vec<String> = slots
            .iter()
            .map(|((name, root), start)| {
                let state = match start.peek() {
                    None => "starting".to_string(),
                    Some(Ok(client)) if client.is_alive() => "running".to_string(),
                    Some(Ok(_)) => "exited".to_string(),
                    Some(Err(error)) => format!("failed: {error}"),
                };
                format!("{name} ({}): {state}", root.display())
            })
            .collect();
        lines.sort();
        lines
    }
}

/// `path` with symlinks resolved, so one file always maps to the same server root; a file that does
/// not exist yet resolves through its parent directory.
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize()
        .ok()
        .or_else(|| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?)))
        .unwrap_or_else(|| path.to_path_buf())
}

async fn start_client(
    launcher: Arc<dyn Launcher>,
    server: ServerConfig,
    command: PathBuf,
    root: PathBuf,
) -> Result<Arc<LspClient>, String> {
    let launched = launcher.launch(&server, &command, &root)?;
    let stderr = launched.stderr.clone();
    let spec = ClientSpec {
        name: &server.name,
        root: &root,
        initialization_options: server.initialization_options.clone(),
        settings: server.settings.clone(),
    };
    match LspClient::start(spec, launched.reader, launched.writer, launched.process).await {
        Ok(client) => Ok(Arc::new(client)),
        Err(error) => {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(200);
            while stderr.last_lines(1).is_empty() && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            let output = stderr.last_lines(3);
            let output = if output.is_empty() {
                String::new()
            } else {
                format!("; stderr: {output}")
            };
            Err(format!("{} failed to initialize: {error}{output}", server.name))
        }
    }
}
