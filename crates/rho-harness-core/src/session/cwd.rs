use super::fs::run_blocking;
use crate::error::{AppError, Result};
use crate::fs::write_atomic;
use crate::locking::lock_file_within;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::Duration;

type LastSessions = BTreeMap<String, String>;

const INDEX_FILE: &str = ".last_sessions.json";
const SESSION_CWDS_FILE: &str = ".session_cwds.json";
const INDEX_LOCK_FILE: &str = ".session_index.lock";
const INDEX_LOCK_TIMEOUT: Duration = Duration::from_secs(5);

fn parse_index(content: &str, index_file: &Path) -> Result<LastSessions> {
    serde_json::from_str(content)
        .map_err(|e| AppError::Session(format!("Failed to parse {}: {e}", index_file.display())))
}

fn encode_index(map: &LastSessions) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(map).map_err(|e| AppError::Session(e.to_string()))
}

fn index_or_empty(content: std::io::Result<String>, index_file: &Path) -> Result<LastSessions> {
    match content {
        Ok(content) => parse_index(&content, index_file),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LastSessions::new()),
        Err(e) => Err(e.into()),
    }
}

fn read_index(index_file: &Path) -> Result<LastSessions> {
    index_or_empty(std::fs::read_to_string(index_file), index_file)
}

fn cwd_key(canonical: std::io::Result<PathBuf>, cwd: &Path) -> String {
    canonical.unwrap_or_else(|_| cwd.to_path_buf()).display().to_string()
}

fn update_index(index_file: &Path, edit: impl FnOnce(&mut LastSessions)) -> Result<()> {
    let mut map = read_index(index_file)?;
    edit(&mut map);
    write_atomic(index_file, &encode_index(&map)?, 0o600)?;
    Ok(())
}

/// Runs `edit` on both index files while holding a lock on the sessions directory, so concurrent
/// writers cannot lose each other's read-modify-write updates.
fn update_indexes(sessions_dir: &Path, timeout: Duration, edit: impl FnOnce() -> Result<()>) -> Result<()> {
    let lock_path = sessions_dir.join(INDEX_LOCK_FILE);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)?;
    if !lock_file_within(&lock, timeout)? {
        return Err(AppError::Session(format!(
            "timed out waiting for the session index lock {}",
            lock_path.display()
        )));
    }
    edit()
}

pub fn record_session_for_cwd(sessions_dir: &Path, cwd: &Path, session_id: &str) -> Result<()> {
    std::fs::create_dir_all(sessions_dir)?;
    let key = cwd_key(std::fs::canonicalize(cwd), cwd);
    update_indexes(sessions_dir, INDEX_LOCK_TIMEOUT, || {
        update_index(&sessions_dir.join(INDEX_FILE), |map| {
            map.insert(key.clone(), session_id.to_string());
        })?;
        update_index(&sessions_dir.join(SESSION_CWDS_FILE), |map| {
            map.insert(session_id.to_string(), key);
        })
    })
}

/// Drops `session_ids` from both indexes, including as the last session of any working directory.
fn forget_sessions(sessions_dir: &Path, session_ids: &BTreeSet<String>) -> Result<()> {
    if session_ids.is_empty() {
        return Ok(());
    }
    update_indexes(sessions_dir, INDEX_LOCK_TIMEOUT, || {
        update_index(&sessions_dir.join(INDEX_FILE), |map| {
            map.retain(|_, last| !session_ids.contains(last))
        })?;
        update_index(&sessions_dir.join(SESSION_CWDS_FILE), |map| {
            map.retain(|id, _| !session_ids.contains(id))
        })
    })
}

pub async fn forget_sessions_async(sessions_dir: &Path, session_ids: BTreeSet<String>) -> Result<()> {
    let sessions_dir = sessions_dir.to_path_buf();
    run_blocking(move || forget_sessions(&sessions_dir, &session_ids)).await
}

pub fn last_session_for_cwd(sessions_dir: &Path, cwd: &Path) -> Result<Option<String>> {
    let map = read_index(&sessions_dir.join(INDEX_FILE))?;
    Ok(map.get(&cwd_key(std::fs::canonicalize(cwd), cwd)).cloned())
}

pub async fn last_session_for_cwd_async(sessions_dir: &Path, cwd: &Path) -> Result<Option<String>> {
    let (sessions_dir, cwd) = (sessions_dir.to_path_buf(), cwd.to_path_buf());
    run_blocking(move || last_session_for_cwd(&sessions_dir, &cwd)).await
}

pub fn cwd_for_session(sessions_dir: &Path, session_id: &str) -> Result<Option<PathBuf>> {
    let map = read_index(&sessions_dir.join(SESSION_CWDS_FILE))?;
    Ok(map.get(session_id).map(PathBuf::from))
}

pub async fn cwd_for_session_async(sessions_dir: &Path, session_id: &str) -> Result<Option<PathBuf>> {
    let (sessions_dir, session_id) = (sessions_dir.to_path_buf(), session_id.to_string());
    run_blocking(move || cwd_for_session(&sessions_dir, &session_id)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_update_fails_at_the_deadline_while_another_holder_keeps_the_lock() {
        let dir = crate::test_support::temp_dir();
        let held = std::fs::File::create(dir.join(INDEX_LOCK_FILE)).unwrap();
        held.lock().unwrap();

        let error = update_indexes(&dir, Duration::ZERO, || Ok(())).unwrap_err();

        assert!(error.to_string().contains("timed out"), "{error}");
        held.unlock().unwrap();
        update_indexes(&dir, Duration::ZERO, || Ok(())).unwrap();
    }

    #[tokio::test]
    async fn forgetting_several_sessions_drops_each_from_both_indexes_and_keeps_others() {
        let dir = crate::test_support::temp_dir();
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        record_session_for_cwd(&dir, &a, "sess-a").unwrap();
        record_session_for_cwd(&dir, &b, "sess-b").unwrap();
        record_session_for_cwd(&dir, &dir, "sess-kept").unwrap();

        let forgotten = BTreeSet::from(["sess-a".to_string(), "sess-b".to_string()]);
        forget_sessions_async(&dir, forgotten).await.unwrap();

        assert_eq!(last_session_for_cwd(&dir, &a).unwrap(), None);
        assert_eq!(last_session_for_cwd(&dir, &b).unwrap(), None);
        assert_eq!(cwd_for_session(&dir, "sess-a").unwrap(), None);
        assert_eq!(cwd_for_session(&dir, "sess-b").unwrap(), None);
        assert_eq!(last_session_for_cwd(&dir, &dir).unwrap().as_deref(), Some("sess-kept"));
    }
}
