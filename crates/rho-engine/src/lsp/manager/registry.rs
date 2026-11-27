//! The process-wide registry of managers, one per workspace, and shutting them down.

use super::{LspManager, SHUTDOWN_BUDGET};
use crate::lsp::support::lock;
use crate::paths::canonical_or_parent;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

static MANAGERS: LazyLock<Mutex<HashMap<PathBuf, Arc<LspManager>>>> = LazyLock::new(Default::default);

/// The manager registered for `workspace` (by its resolved path), made by `init` the first time.
pub(super) fn managed(workspace: &Path, init: impl FnOnce() -> Arc<LspManager>) -> Arc<LspManager> {
    let mut managers = lock(&MANAGERS);
    Arc::clone(managers.entry(canonical_or_parent(workspace)).or_insert_with(init))
}

/// Shuts down the servers of `workspace` and forgets its manager, so a later [`LspManager::for_workspace`]
/// starts fresh. Does nothing for a workspace without a manager.
pub async fn shutdown_workspace(workspace: &Path) {
    if let Some(manager) = forget_workspace(workspace) {
        manager.shutdown_all().await;
    }
}

/// Drops the registry's hold on the manager of `workspace`. Without other holders its clients drop,
/// and dropping a client kills its server's process group; usable where nothing can be awaited.
pub fn forget_workspace(workspace: &Path) -> Option<Arc<LspManager>> {
    lock(&MANAGERS).remove(&canonical_or_parent(workspace))
}

#[cfg(test)]
pub(crate) fn register_manager(manager: &Arc<LspManager>) {
    lock(&MANAGERS).insert(manager.workspace().to_path_buf(), Arc::clone(manager));
}

#[cfg(test)]
pub(crate) fn is_registered(workspace: &Path) -> bool {
    lock(&MANAGERS).contains_key(&canonical_or_parent(workspace))
}

/// Shuts down the servers of every workspace in this process.
pub async fn shutdown_all_managers() {
    let managers: Vec<Arc<LspManager>> = lock(&MANAGERS).values().cloned().collect();
    futures::future::join_all(managers.iter().map(|manager| manager.shutdown_all())).await;
}

/// [`shutdown_all_managers`] for code that cannot await, such as a `Drop` or an exit path. Needs a
/// multi-threaded runtime to run the shutdown; on any other runtime it does nothing.
pub fn shutdown_all_blocking() {
    block_on_runtime(shutdown_all_managers(), SHUTDOWN_BUDGET + Duration::from_secs(1));
}

/// Runs `work` on the current runtime from a separate thread and waits up to `budget` for it. Waiting
/// on a task spawned from a runtime worker can stall: that worker is blocked, and a task in its
/// scheduler slot cannot be stolen.
pub(super) fn block_on_runtime(work: impl Future<Output = ()> + Send + 'static, budget: Duration) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    if handle.runtime_flavor() != tokio::runtime::RuntimeFlavor::MultiThread {
        return;
    }
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        handle.block_on(work);
        let _ = done.send(());
    });
    let _ = finished.recv_timeout(budget);
}
