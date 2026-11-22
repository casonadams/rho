//! Starts language servers on demand and routes each file to the servers that handle it.

use super::client::{ClientSpec, LspClient};
use super::config::{ServerConfig, ServerSet, layer_paths, load_servers, resolve_command};
pub use super::process::{Launched, Launcher, ProcessLauncher};
use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};
use tokio::time::Instant;

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

type StartFuture = Shared<BoxFuture<'static, Result<Arc<LspClient>, String>>>;
type Key = (String, PathBuf);
type Slots = HashMap<Key, Slot>;

struct Slot {
    start: StartFuture,
    started: Instant,
    /// Replaces a client that died, so a second death soon after is not restarted at once.
    respawn: bool,
}

impl Slot {
    fn client(&self) -> Option<&Arc<LspClient>> {
        match self.start.peek() {
            Some(Ok(client)) => Some(client),
            _ => None,
        }
    }

    fn running(&self) -> Option<&Arc<LspClient>> {
        self.client().filter(|client| client.is_alive())
    }
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

enum Reuse {
    Keep,
    Backoff(String),
    Replace { respawn: bool },
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
    reaper: AtomicBool,
    reported_failures: Mutex<HashSet<Key>>,
    pub(super) epochs: super::watch::Epochs,
}

static MANAGERS: LazyLock<Mutex<HashMap<PathBuf, Arc<LspManager>>>> = LazyLock::new(Default::default);

pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
            reaper: AtomicBool::new(false),
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
        resolve_command(&server.command, self.path_var.as_deref()).is_some()
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
        let command = resolve_command(&server.command, self.path_var.as_deref())?;
        let root = match path.starts_with(&self.workspace) {
            true => server.root_for(path, &self.workspace)?,
            false => self.running_root(&server.name)?,
        };
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

    fn reuse(existing: &Slot) -> Reuse {
        match existing.start.peek() {
            Some(Ok(client)) if client.is_alive() => Reuse::Keep,
            Some(Ok(_)) if existing.respawn && existing.started.elapsed() < RETRY_FAILED_AFTER => {
                let wait = RETRY_FAILED_AFTER.saturating_sub(existing.started.elapsed());
                Reuse::Backoff(format!(
                    "the language server exited again right after a restart; not restarting it for {}s",
                    wait.as_secs() + 1
                ))
            }
            Some(Ok(_)) => Reuse::Replace { respawn: true },
            Some(Err(_)) if existing.started.elapsed() < RETRY_FAILED_AFTER => Reuse::Keep,
            Some(Err(_)) => Reuse::Replace { respawn: false },
            None => Reuse::Keep,
        }
    }

    fn slot(&self, candidate: &Candidate) -> StartFuture {
        let key = candidate.key();
        let mut slots = lock(&self.slots);
        let mut respawn = false;
        if let Some(existing) = slots.get(&key) {
            match Self::reuse(existing) {
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
        if self.reaper.swap(true, Ordering::SeqCst) {
            return;
        }
        let slots = Arc::downgrade(&self.slots);
        let idle_ms = Arc::clone(&self.idle_ms);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(REAP_INTERVAL).await;
                let Some(slots) = slots.upgrade() else {
                    return;
                };
                reap_idle(&slots, Duration::from_millis(idle_ms.load(Ordering::SeqCst))).await;
            }
        });
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
        self.clients_within_hinting(path, wait, &retry).await
    }

    /// [`LspManager::clients_within`], with `retry` as the advice for a server still starting.
    pub async fn clients_within_hinting(
        &self,
        path: &Path,
        wait: Duration,
        retry: &str,
    ) -> Vec<Result<Arc<LspClient>, String>> {
        let starts: Vec<(Key, StartFuture)> = self
            .candidates(path)
            .iter()
            .filter_map(|c| {
                let start = if self.trusted {
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
            .filter(|(failed, _)| failed.as_ref().is_none_or(|key| reported.insert(key.clone())))
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

/// The manager registered for `workspace` (by its resolved path), made by `init` the first time.
fn managed(workspace: &Path, init: impl FnOnce() -> Arc<LspManager>) -> Arc<LspManager> {
    let mut managers = lock(&MANAGERS);
    Arc::clone(managers.entry(canonical(workspace)).or_insert_with(init))
}

/// Shuts down the servers of `workspace` and forgets its manager, so a later [`LspManager::for_workspace`]
/// starts fresh. Does nothing for a workspace without a manager.
pub async fn shutdown_workspace(workspace: &Path) {
    let manager = lock(&MANAGERS).remove(&canonical(workspace));
    if let Some(manager) = manager {
        manager.shutdown_all().await;
    }
}

#[cfg(test)]
pub(crate) fn register_manager(manager: &Arc<LspManager>) {
    lock(&MANAGERS).insert(manager.workspace.clone(), Arc::clone(manager));
}

#[cfg(test)]
pub(crate) fn is_registered(workspace: &Path) -> bool {
    lock(&MANAGERS).contains_key(&canonical(workspace))
}

/// Shuts down the servers of every workspace in this process.
pub async fn shutdown_all_managers() {
    let managers: Vec<Arc<LspManager>> = lock(&MANAGERS).values().cloned().collect();
    futures::future::join_all(managers.iter().map(|manager| manager.shutdown_all())).await;
}

/// [`shutdown_all_managers`] for code that cannot await, such as a `Drop` or an exit path. Needs a
/// multi-threaded runtime to run the shutdown; on any other runtime it does nothing.
pub fn shutdown_all_blocking() {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    if handle.runtime_flavor() != tokio::runtime::RuntimeFlavor::MultiThread {
        return;
    }
    let (done, finished) = std::sync::mpsc::channel();
    handle.spawn(async move {
        shutdown_all_managers().await;
        let _ = done.send(());
    });
    let _ = finished.recv_timeout(SHUTDOWN_BUDGET + Duration::from_secs(1));
}

/// Forgets servers that exited long enough ago that restarting one is no longer held back.
fn prune_exited(slots: &mut Slots) {
    slots.retain(|_, slot| {
        slot.running().is_some() || slot.client().is_none() || slot.started.elapsed() < RETRY_FAILED_AFTER
    });
}

/// Stops the server that has been idle longest when the cap on running servers is reached.
fn stop_least_recently_used(slots: &mut Slots) {
    let running = slots.iter().filter(|(_, slot)| slot.running().is_some()).count();
    if running < MAX_RUNNING_SERVERS {
        return;
    }
    let idlest = slots
        .iter()
        .filter_map(|(key, slot)| Some((key, slot.running()?.idle_for())))
        .max_by_key(|(_, idle)| *idle)
        .map(|(key, _)| key.clone());
    let stopped = idlest
        .and_then(|key| slots.remove(&key))
        .and_then(|slot| slot.running().cloned());
    if let Some(client) = stopped {
        tokio::spawn(async move { client.shutdown().await });
    }
}

/// Shuts down the servers unused for `idle`; zero keeps them forever.
async fn reap_idle(slots: &Mutex<Slots>, idle: Duration) {
    if idle.is_zero() {
        return;
    }
    let stale: Vec<Arc<LspClient>> = {
        let mut slots = lock(slots);
        let keys: Vec<Key> = slots
            .iter()
            .filter(|(_, slot)| slot.running().is_some_and(|client| client.idle_for() >= idle))
            .map(|(key, _)| key.clone())
            .collect();
        keys.iter()
            .filter_map(|key| slots.remove(key))
            .filter_map(|slot| slot.running().cloned())
            .collect()
    };
    futures::future::join_all(stale.iter().map(|client| client.shutdown())).await;
}

/// `path` with symlinks resolved, so one file always maps to the same server root; a file that does
/// not exist yet resolves through its parent directory.
pub(super) fn canonical(path: &Path) -> PathBuf {
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
            let deadline = Instant::now() + Duration::from_millis(200);
            while stderr.last_lines(1).is_empty() && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(10)).await;
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

#[cfg(test)]
mod tests;
