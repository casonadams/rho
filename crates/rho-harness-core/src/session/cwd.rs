use super::fs::run_blocking;
use crate::error::{AppError, Result};
use crate::fs::write_atomic;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type LastSessions = BTreeMap<String, String>;

const INDEX_FILE: &str = ".last_sessions.json";
const SESSION_CWDS_FILE: &str = ".session_cwds.json";

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

pub fn record_session_for_cwd(sessions_dir: &Path, cwd: &Path, session_id: &str) -> Result<()> {
    std::fs::create_dir_all(sessions_dir)?;
    let key = cwd_key(std::fs::canonicalize(cwd), cwd);

    let index_file = sessions_dir.join(INDEX_FILE);
    let mut map = read_index(&index_file)?;
    map.insert(key.clone(), session_id.to_string());
    write_atomic(&index_file, &encode_index(&map)?, 0o600)?;

    let session_cwds_file = sessions_dir.join(SESSION_CWDS_FILE);
    let mut session_cwds = read_index(&session_cwds_file)?;
    session_cwds.insert(session_id.to_string(), key);
    write_atomic(&session_cwds_file, &encode_index(&session_cwds)?, 0o600)?;
    Ok(())
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
