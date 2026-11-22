//! Starts language servers on demand and routes each file to the servers that handle it.

mod registry;
mod slots;

use super::client::LspClient;
use super::config::{ServerConfig, ServerSet, layer_paths, load_servers, resolve_command};
pub use super::process::{Launched, Launcher, ProcessLauncher};
use super::support::{canonical, lock};
use futures::FutureExt;
use slots::{
    Key, Reuse, Slot, Slots, StartFuture, first_report, prune_exited, reap_idle, start_client, stop_least_recently_used,
};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::time::Instant;

#[cfg(test)]
use registry::block_on_runtime;
use registry::managed;
pub use registry::{forget_workspace, shutdown_all_blocking, shutdown_all_managers, shutdown_workspace};
#[cfg(test)]
pub(crate) use registry::{is_registered, register_manager};

/// How long a server that failed to start, or died right after a restart, waits before the next
/// use tries again.
pub const RETRY_FAILED_AFTER: Duration = Duration::from_secs(60);
/// Servers running at once; starting another stops the least recently used.
pub const MAX_RUNNING_SERVERS: usize = 8;
/// How long a server may sit unused before it is shut down.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The most time a shutdown of every server may take.
pub const SHUTDOWN_BUDGET: Duration = Duration::from_secs(3);
const REAP_INTERVAL: Duration = Duration::from_secs(30);
const CONFIG_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// Who is asking for servers: only the model may start them in a project that is not trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Initiator {
    Automatic,
    Model,
}

/// A server that handles a file, with the root it runs in.
pub struct Candidate {
    pub server: ServerConfig,
    pub command: PathBuf,
    pub root: PathBuf,
}

impl Candidate {
    fn key(&self) -> Key {
        (self.server.name.clone(), self.root.clone())
    }
}

#[derive(Default)]
struct Loaded {
    servers: Vec<ServerConfig>,
    config_error: Option<String>,
    notice: Option<String>,
}

/// The `lsp.json` files' modification times, to notice an edit while rho runs.
struct Watch {
    home: Option<PathBuf>,
    trusted: bool,
    stamps: Vec<Option<(SystemTime, u64)>>,
    checked: Option<Instant>,
}

fn stamps(paths: &[PathBuf]) -> Vec<Option<(SystemTime, u64)>> {
    paths
        .iter()
        .map(|path| {
            let meta = std::fs::metadata(path).ok()?;
            Some((meta.modified().ok()?, meta.len()))
        })
        .collect()
}

pub struct LspManager {
    workspace: PathBuf,
    loaded: Mutex<Arc<Loaded>>,
    watch: Option<Mutex<Watch>>,
    idle_ms: Arc<AtomicU64>,
    trusted: bool,
    path_var: Option<OsString>,
    launcher: Arc<dyn Launcher>,
    slots: Arc<Mutex<Slots>>,
    reaper: Mutex<Option<tokio::task::JoinHandle<()>>>,
    reported_failures: Mutex<HashSet<Key>>,
    pub(super) epochs: super::watch::Epochs,
}

fn loaded_from(result: Result<ServerSet, String>) -> (Loaded, u64) {
    let (set, config_error) = match result {
        Ok(set) => (set, None),
        Err(error) => (ServerSet::default(), Some(error)),
    };
    let idle_ms = set.idle_timeout_ms.unwrap_or(DEFAULT_IDLE_TIMEOUT.as_millis() as u64);
    let loaded = Loaded {
        servers: set.servers,
        config_error,
        notice: set.notice,
    };
    (loaded, idle_ms)
}

impl LspManager {
    /// A manager that starts servers only on explicit request; see [`LspManager::with_trust`].
    pub fn new(
        workspace: &Path,
        servers: Result<ServerSet, String>,
        path_var: Option<OsString>,
        launcher: Arc<dyn Launcher>,
    ) -> Self {
        let (loaded, idle_ms) = loaded_from(servers);
        Self {
            workspace: canonical(workspace),
            loaded: Mutex::new(Arc::new(loaded)),
            watch: None,
            idle_ms: Arc::new(AtomicU64::new(idle_ms)),
            trusted: false,
            path_var,
            launcher,
            slots: Arc::default(),
            reaper: Mutex::new(None),
            reported_failures: Mutex::new(HashSet::new()),
            epochs: Default::default(),
        }
    }

    /// Whether reading a file and editing it may start servers on their own. In a project that is
    /// not trusted, servers start only when asked for (the `lsp` tool, `read` with a symbol).
    pub fn with_trust(mut self, trusted: bool) -> Self {
        self.trusted = trusted;
        self
    }

    /// Re-reads the `lsp.json` files when one changes, or after one failed to load.
    pub fn with_reload(mut self, home: Option<PathBuf>, trusted: bool) -> Self {
        let paths = layer_paths(home.as_deref(), &self.workspace);
        self.watch = Some(Mutex::new(Watch {
            home,
            trusted,
            stamps: stamps(&paths),
            checked: Some(Instant::now()),
        }));
        self
    }

    /// The process-wide manager for `workspace`, so engine rebuilds keep their running servers.
    pub fn for_workspace(workspace: &Path) -> Arc<Self> {
        managed(workspace, || {
            let home = rho_harness_core::config::home_dir();
            let trusted = crate::mcp::trust::project_trusted_by_env();
            Arc::new(
                Self::new(
                    workspace,
                    load_servers(home.as_deref(), workspace, trusted),
                    std::env::var_os("PATH"),
                    Arc::new(ProcessLauncher),
                )
                .with_trust(trusted)
                .with_reload(home, trusted),
            )
        })
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn in_workspace(&self, path: &Path) -> bool {
        canonical(path).starts_with(&self.workspace)
    }

    /// `path` relative to the workspace, as the `lsp` tool takes it.
    pub fn relative(&self, path: &Path) -> String {
        let path = canonical(path);
        path.strip_prefix(&self.workspace)
            .unwrap_or(&path)
            .display()
            .to_string()
    }

    fn current(&self) -> Arc<Loaded> {
        if let Some(watch) = &self.watch {
            self.refresh(&mut lock(watch));
        }
        Arc::clone(&lock(&self.loaded))
    }

    fn refresh(&self, watch: &mut Watch) {
        if watch.checked.is_some_and(|at| at.elapsed() < CONFIG_CHECK_INTERVAL) {
            return;
        }
        watch.checked = Some(Instant::now());
        let current = stamps(&layer_paths(watch.home.as_deref(), &self.workspace));
        let failed = lock(&self.loaded).config_error.is_some();
        if current == watch.stamps && !failed {
            return;
        }
        watch.stamps = current;
        let (loaded, idle_ms) = loaded_from(load_servers(watch.home.as_deref(), &self.workspace, watch.trusted));
        self.idle_ms.store(idle_ms, Ordering::SeqCst);
        *lock(&self.loaded) = Arc::new(loaded);
    }

    pub fn servers(&self) -> Vec<ServerConfig> {
        self.current().servers.clone()
    }

    pub fn notice(&self) -> Option<String> {
        self.current().notice.clone()
    }

    pub fn is_installed(&self, server: &ServerConfig) -> bool {
        resolve_command(&server.command, self.path_var.as_deref(), &self.workspace).is_some()
    }

    /// The root of a running server named `name`: a file outside the workspace (a dependency the
    /// model jumped into) is answered by the server already running, never by a new one.
    fn running_root(&self, name: &str) -> Option<PathBuf> {
        let slots = lock(&self.slots);
        let mut roots: Vec<&PathBuf> = slots
            .iter()
            .filter(|((server, _), slot)| server == name && slot.running().is_some())
            .map(|((_, root), _)| root)
            .collect();
        roots.sort();
        roots.first().map(|root| (*root).clone())
    }

    fn candidate(&self, server: &ServerConfig, path: &Path) -> Option<Candidate> {
        let root = match path.starts_with(&self.workspace) {
            true => server.root_for(path, &self.workspace)?,
            false => self.running_root(&server.name)?,
        };
        let command = resolve_command(&server.command, self.path_var.as_deref(), &root)?;
        Some(Candidate {
            server: server.clone(),
            command,
            root,
        })
    }

    /// Servers that can run for `path`: the main one first, then linters.
    pub fn candidates(&self, path: &Path) -> Vec<Candidate> {
        let path = canonical(path);
        let path = path.as_path();
        let loaded = self.current();
        let mut found = Vec::new();
        let mut has_main = false;
        for server in loaded.servers.iter().filter(|s| s.handles(path)) {
            if (server.linter || !has_main)
                && let Some(candidate) = self.candidate(server, path)
            {
                has_main |= !server.linter;
                found.push(candidate);
            }
        }
        found.sort_by_key(|candidate| candidate.server.linter);
        found
    }

    /// Why no server can answer for `path`.
    pub fn unavailable_reason(&self, path: &Path) -> String {
        let loaded = self.current();
        if let Some(error) = &loaded.config_error {
            return format!("lsp.json is invalid: {error}");
        }
        if let Some(failure) = self.start_failure(path) {
            return failure;
        }
        let notice = loaded.notice.as_deref().map(|n| format!(" ({n})")).unwrap_or_default();
        let handlers: Vec<&ServerConfig> = loaded.servers.iter().filter(|s| s.handles(path)).collect();
        if handlers.is_empty() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            return format!("no language server is configured for '.{ext}' files{notice}");
        }
        let outside = !canonical(path).starts_with(&self.workspace);
        let reasons: Vec<String> = handlers
            .iter()
            .map(|server| match (self.is_installed(server), outside) {
                (false, _) => format!("{}: `{}` is not on PATH", server.name, server.command),
                (true, true) => format!(
                    "{}: the file is outside the workspace and no server is running",
                    server.name
                ),
                (true, false) => format!(
                    "{}: no root marker ({}) above the file",
                    server.name,
                    server.root_markers.join(", ")
                ),
            })
            .collect();
        format!("no language server available: {}{notice}", reasons.join("; "))
    }

    fn start_failure(&self, path: &Path) -> Option<String> {
        let keys: Vec<Key> = self.candidates(path).iter().map(Candidate::key).collect();
        let slots = lock(&self.slots);
        keys.iter().find_map(|key| match slots.get(key)?.start.peek() {
            Some(Err(error)) => Some(error.clone()),
            _ => None,
        })
    }

    fn slot(&self, candidate: &Candidate) -> StartFuture {
        let key = candidate.key();
        let mut slots = lock(&self.slots);
        let mut respawn = false;
        if let Some(existing) = slots.get(&key) {
            match existing.reuse() {
                Reuse::Keep => {
                    if let Some(client) = existing.running() {
                        client.touch();
                    }
                    return existing.start.clone();
                }
                Reuse::Backoff(reason) => return futures::future::ready(Err(reason)).boxed().shared(),
                Reuse::Replace { respawn: after_death } => respawn = after_death,
            }
            lock(&self.reported_failures).remove(&key);
        }
        prune_exited(&mut slots);
        stop_least_recently_used(&mut slots);
        let start = start_client(
            Arc::clone(&self.launcher),
            candidate.server.clone(),
            candidate.command.clone(),
            candidate.root.clone(),
        )
        .boxed()
        .shared();
        tokio::spawn(start.clone().map(|_| ()));
        let slot = Slot {
            start: start.clone(),
            started: Instant::now(),
            respawn,
        };
        slots.insert(key, slot);
        drop(slots);
        self.ensure_reaper();
        start
    }

    fn ensure_reaper(&self) {
        let mut reaper = lock(&self.reaper);
        if reaper.as_ref().is_some_and(|task| !task.is_finished()) {
            return;
        }
        let slots = Arc::downgrade(&self.slots);
        let idle_ms = Arc::clone(&self.idle_ms);
        *reaper = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(REAP_INTERVAL).await;
                let Some(slots) = slots.upgrade() else {
                    return;
                };
                reap_idle(&slots, Duration::from_millis(idle_ms.load(Ordering::SeqCst))).await;
            }
        }));
    }

    /// The slot already holding a server (running or still starting) for `key`.
    fn existing(&self, key: &Key) -> Option<StartFuture> {
        let slots = lock(&self.slots);
        let slot = slots.get(key)?;
        (!matches!(slot.start.peek(), Some(Err(_)))).then(|| slot.start.clone())
    }

    /// Starts (or reuses) every server for `path` without waiting for them. Starts nothing in a
    /// project that is not trusted.
    pub fn warm(&self, path: &Path) {
        if !self.trusted {
            return;
        }
        for candidate in self.candidates(path) {
            drop(self.slot(&candidate));
        }
    }

    /// The main server for `path`, started if needed (an explicit request, trusted or not).
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
    /// to start is reported once per attempt, then left out. In a project that is not trusted only
    /// servers that already run are used.
    pub async fn clients_within(&self, path: &Path, wait: Duration) -> Vec<Result<Arc<LspClient>, String>> {
        let retry = format!("run lsp diagnostics path={} in a moment", self.relative(path));
        self.clients_within_hinting(path, wait, &retry, Initiator::Automatic)
            .await
    }

    /// [`LspManager::clients_within`], with `retry` as the advice for a server still starting. A
    /// request from the `Model` may start servers even in a project that is not trusted.
    pub async fn clients_within_hinting(
        &self,
        path: &Path,
        wait: Duration,
        retry: &str,
        initiator: Initiator,
    ) -> Vec<Result<Arc<LspClient>, String>> {
        let starts: Vec<(Key, StartFuture)> = self
            .candidates(path)
            .iter()
            .filter_map(|c| {
                let start = if self.trusted || initiator == Initiator::Model {
                    Some(self.slot(c))
                } else {
                    self.existing(&c.key())
                };
                Some((c.key(), start?))
            })
            .collect();
        let waits = starts.into_iter().map(|(key, start)| async move {
            match tokio::time::timeout(wait, start).await {
                Ok(Err(error)) => (Some(key), Err(error)),
                Ok(Ok(client)) => (None, Ok(client)),
                Err(_) => (None, Err(format!("{} is still starting; {retry}", key.0))),
            }
        });
        let results = futures::future::join_all(waits).await;
        let mut reported = lock(&self.reported_failures);
        results
            .into_iter()
            .filter(|(failed, _)| {
                failed
                    .as_ref()
                    .is_none_or(|key| first_report(&mut reported, key.clone()))
            })
            .map(|(_, result)| result)
            .collect()
    }

    /// Clients that finished starting and are still running.
    pub fn running_clients(&self) -> Vec<Arc<LspClient>> {
        lock(&self.slots)
            .values()
            .filter_map(|slot| slot.running().cloned())
            .collect()
    }

    /// The running clients, or when none run, the servers whose root marker is in the workspace root.
    pub async fn workspace_clients(&self) -> Vec<Arc<LspClient>> {
        let running = self.running_clients();
        if !running.is_empty() {
            return running;
        }
        let starts: Vec<StartFuture> = self
            .servers()
            .into_iter()
            .filter(|server| !server.linter && !server.root_markers.is_empty())
            .filter_map(|server| {
                let root = server.root_for(&self.workspace.join("_"), &self.workspace)?;
                let command = resolve_command(&server.command, self.path_var.as_deref(), &root)?;
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
        let mut slots = lock(&self.slots);
        prune_exited(&mut slots);
        let mut lines: Vec<String> = slots
            .iter()
            .map(|((name, root), slot)| {
                let state = match slot.start.peek() {
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

    /// Shuts every running server down, within [`SHUTDOWN_BUDGET`] in total; whatever is left is killed.
    pub async fn shutdown_all(&self) {
        let clients: Vec<Arc<LspClient>> = lock(&self.slots)
            .drain()
            .filter_map(|(_, slot)| slot.running().cloned())
            .collect();
        let _ = tokio::time::timeout(
            SHUTDOWN_BUDGET,
            futures::future::join_all(clients.iter().map(|client| client.shutdown())),
        )
        .await;
    }
}

#[cfg(test)]
mod reload_tests;
#[cfg(test)]
mod tests;
