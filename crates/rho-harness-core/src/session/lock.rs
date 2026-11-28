use super::format::StoreState;
use super::fs::session_error;
use super::tree::{SessionTree, TitleSource};
use crate::error::Result;
use crate::locking::{lock, read, write};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

/// The session name and its source, readable without waiting for the session state mutex.
#[derive(Debug, Default)]
pub(crate) struct TitleCell(RwLock<(Option<String>, Option<TitleSource>)>);

impl TitleCell {
    fn of(tree: &SessionTree) -> Self {
        let cell = Self::default();
        cell.publish(tree);
        cell
    }

    pub(crate) fn publish(&self, tree: &SessionTree) {
        let mut title = write(&self.0);
        if title.0 != tree.session_name || title.1 != tree.session_name_source {
            *title = (tree.session_name.clone(), tree.session_name_source);
        }
    }

    pub(crate) fn name(&self) -> Option<String> {
        read(&self.0).0.clone()
    }

    pub(crate) fn source(&self) -> Option<TitleSource> {
        read(&self.0).1
    }
}

struct Entry {
    file: File,
    holders: usize,
    state: Arc<tokio::sync::Mutex<StoreState>>,
    title: Arc<TitleCell>,
}

impl Drop for Entry {
    /// A child forked by another thread inherits this descriptor until it execs, and the lock
    /// lives on the shared open file description, so closing alone would leave it held.
    fn drop(&mut self) {
        drop(self.file.unlock());
    }
}

/// Open handles and in-memory state keyed by session path. Managers in this process share one
/// handle and one state, so a session can be re-opened while its previous engine is still alive
/// without two sequence counters writing to the same file; the OS lock excludes other processes.
/// Holder counts change only under this mutex, so the file closes before another opener can
/// observe the entry as gone.
static HELD: Mutex<BTreeMap<PathBuf, Entry>> = Mutex::new(BTreeMap::new());

fn held() -> MutexGuard<'static, BTreeMap<PathBuf, Entry>> {
    lock(&HELD)
}

pub(crate) struct SessionLock {
    path: PathBuf,
    state: Arc<tokio::sync::Mutex<StoreState>>,
    title: Arc<TitleCell>,
}

impl SessionLock {
    pub(crate) fn state(&self) -> &Arc<tokio::sync::Mutex<StoreState>> {
        &self.state
    }

    pub(crate) fn title(&self) -> &Arc<TitleCell> {
        &self.title
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        let mut held = held();
        if let Some(entry) = held.get_mut(&self.path) {
            entry.holders -= 1;
            if entry.holders == 0 {
                held.remove(&self.path);
            }
        }
    }
}

/// Registers a holder for `path`. Later holders in this process adopt the first holder's state;
/// only the first, once it holds the OS lock, runs `load`.
pub(crate) fn acquire(path: &Path, load: impl FnOnce() -> Result<StoreState>) -> Result<Arc<SessionLock>> {
    let mut held = held();
    if let Some(entry) = held.get_mut(path) {
        entry.holders += 1;
        return Ok(Arc::new(SessionLock {
            path: path.to_path_buf(),
            state: Arc::clone(&entry.state),
            title: Arc::clone(&entry.title),
        }));
    }
    let file = OpenOptions::new().read(true).append(true).open(path)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            return Err(session_error("session is already open in another rho process"));
        }
        Err(TryLockError::Error(error)) => return Err(error.into()),
    }
    let loaded = match load() {
        Ok(loaded) => loaded,
        Err(error) => {
            drop(file.unlock());
            return Err(error);
        }
    };
    let title = Arc::new(TitleCell::of(&loaded.tree));
    let state = Arc::new(tokio::sync::Mutex::new(loaded));
    let entry = Entry {
        file,
        holders: 1,
        state: Arc::clone(&state),
        title: Arc::clone(&title),
    };
    held.insert(path.to_path_buf(), entry);
    Ok(Arc::new(SessionLock {
        path: path.to_path_buf(),
        state,
        title,
    }))
}

/// Removes `path` unless some opener (this or another process) holds its session lock.
/// The lock stays held through the unlink so no opener can slip in between.
pub(crate) fn remove_unless_held(path: &Path) -> bool {
    let held = held();
    if held.contains_key(path) {
        return false;
    }
    let Ok(file) = File::open(path) else {
        return false;
    };
    if file.try_lock().is_err() {
        return false;
    }
    let removed = std::fs::remove_file(path).is_ok();
    drop(file.unlock());
    drop(held);
    removed
}

#[cfg(test)]
mod tests {
    use crate::session::SessionManager;
    use std::fs::{OpenOptions, TryLockError};
    use std::sync::Arc;

    #[tokio::test]
    async fn session_lock_is_exclusive_across_openers_and_shared_in_process() {
        let dir = crate::test_support::temp_dir();
        let owner = SessionManager::new_async(&dir, None).await.unwrap();
        let session_id = owner.session_id.clone();
        let path = owner.file_path.clone();

        let external = OpenOptions::new().read(true).append(true).open(&path).unwrap();
        assert!(matches!(external.try_lock(), Err(TryLockError::WouldBlock)));
        let sharing = SessionManager::new_async(&dir, Some(&session_id)).await;
        assert!(sharing.is_ok());
        drop(sharing);
        drop(owner);
        assert!(external.try_lock().is_ok());
        drop(external);

        let holder = OpenOptions::new().read(true).append(true).open(&path).unwrap();
        holder.try_lock().unwrap();
        assert!(SessionManager::new_async(&dir, Some(&session_id)).await.is_err());
        drop(holder);
        assert!(SessionManager::new_async(&dir, Some(&session_id)).await.is_ok());
    }

    fn append_malformed_record(path: &std::path::Path) {
        use std::io::Write;
        let mut file = OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(b"{malformed}\n").unwrap();
    }

    #[test]
    fn resuming_a_session_locked_elsewhere_is_refused_before_its_file_is_parsed() {
        let dir = crate::test_support::temp_dir();
        let owner = SessionManager::new(&dir, None).unwrap();
        let (session_id, path) = (owner.session_id.clone(), owner.file_path.clone());
        drop(owner);
        append_malformed_record(&path);
        let other_process = external_handle(&path);
        other_process.try_lock().unwrap();

        let error = SessionManager::new(&dir, Some(&session_id)).unwrap_err().to_string();

        assert!(error.contains("already open in another rho process"), "{error}");
        drop(other_process);
        let error = SessionManager::new(&dir, Some(&session_id)).unwrap_err().to_string();
        assert!(error.contains("malformed committed record"), "{error}");
    }

    #[test]
    fn reopening_a_session_held_in_process_adopts_its_state_without_reparsing() {
        let dir = crate::test_support::temp_dir();
        let owner = SessionManager::new(&dir, None).unwrap();
        append_malformed_record(&owner.file_path);

        let shared = SessionManager::new(&dir, Some(&owner.session_id)).unwrap();

        assert!(Arc::ptr_eq(&owner.state, &shared.state));
    }

    #[test]
    fn reopening_while_the_last_holder_drops_never_reports_a_conflict() {
        let dir = crate::test_support::temp_dir();
        let session_id = SessionManager::new(&dir, None).unwrap().session_id.clone();
        for _ in 0..16 {
            let holder = SessionManager::new(&dir, Some(&session_id)).unwrap();
            let dropper = std::thread::spawn(move || drop(holder));
            let reopened = SessionManager::new(&dir, Some(&session_id));
            dropper.join().unwrap();
            assert!(reopened.is_ok());
        }
    }

    fn external_handle(path: &std::path::Path) -> std::fs::File {
        OpenOptions::new().read(true).append(true).open(path).unwrap()
    }

    #[derive(Clone, Copy, Debug)]
    enum Keeper {
        SharedManager,
        ArcClone,
    }

    const KEEPERS: [Keeper; 2] = [Keeper::SharedManager, Keeper::ArcClone];

    #[test]
    fn session_lock_stays_held_while_a_keeper_lives() {
        for keeper in KEEPERS {
            let dir = crate::test_support::temp_dir();
            let owner = SessionManager::new(&dir, None).unwrap();
            let session_id = owner.session_id.clone();
            let path = owner.file_path.clone();
            let (manager, lock) = match keeper {
                Keeper::SharedManager => (Some(SessionManager::new(&dir, Some(&session_id)).unwrap()), None),
                Keeper::ArcClone => (None, Some(owner._session_lock.clone())),
            };
            let external = external_handle(&path);

            drop(owner);
            assert!(
                matches!(external.try_lock(), Err(TryLockError::WouldBlock)),
                "{keeper:?}"
            );
            drop(manager);
            drop(lock);
            assert!(external.try_lock().is_ok(), "{keeper:?}");
        }
    }
}
