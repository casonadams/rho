use super::temp_dir;
use crate::session::prune::{prune_empty_sessions_async, prune_expired_sessions_async};
use crate::session::{SessionManager, delete_session_async};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn workspace(dir: &Path) -> PathBuf {
    let cwd = dir.join("workspace");
    std::fs::create_dir_all(&cwd).unwrap();
    cwd
}

/// Creates a session, indexes it under `cwd`, and releases it so it can be deleted or pruned.
async fn indexed_session(dir: &Path, cwd: &Path) -> (String, PathBuf) {
    let session = SessionManager::new_async(dir, None).await.unwrap();
    let (id, path) = (session.session_id.clone(), session.file_path.clone());
    drop(session);
    SessionManager::record_session_for_cwd(dir, cwd, &id).unwrap();
    (id, path)
}

fn age_file(path: &Path, days: u64) {
    let mtime = SystemTime::now() - Duration::from_secs(days * 86_400 + 3600);
    let file = std::fs::File::open(path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(mtime)).unwrap();
}

fn assert_unindexed(dir: &Path, cwd: &Path, session_id: &str) {
    assert_eq!(SessionManager::last_session_for_cwd(dir, cwd).unwrap(), None);
    assert_eq!(SessionManager::cwd_for_session(dir, session_id).unwrap(), None);
}

#[tokio::test]
async fn deleting_a_session_drops_its_working_directory_entries() {
    let dir = temp_dir();
    let cwd = workspace(&dir);
    let (id, _) = indexed_session(&dir, &cwd).await;

    delete_session_async(&dir, &id).await.unwrap();

    assert_unindexed(&dir, &cwd, &id);
}

#[tokio::test]
async fn pruning_an_empty_session_drops_its_working_directory_entries() {
    let dir = temp_dir();
    let cwd = workspace(&dir);
    let (id, _) = indexed_session(&dir, &cwd).await;

    let pruned = prune_empty_sessions_async(&dir, Some("another-session")).await.unwrap();

    assert_eq!(pruned, 1);
    assert_unindexed(&dir, &cwd, &id);
}

#[tokio::test]
async fn pruning_an_expired_session_drops_its_working_directory_entries() {
    let dir = temp_dir();
    let cwd = workspace(&dir);
    let (id, path) = indexed_session(&dir, &cwd).await;
    age_file(&path, 10);

    let pruned = prune_expired_sessions_async(&dir, "another-session", 5).await.unwrap();

    assert_eq!(pruned, 1);
    assert_unindexed(&dir, &cwd, &id);
}
