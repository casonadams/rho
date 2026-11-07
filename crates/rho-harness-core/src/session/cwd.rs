use super::fs::run_blocking;
use crate::error::{AppError, Result};
use crate::fs::write_atomic;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type LastSessions = BTreeMap<String, String>;

const INDEX_FILE: &str = ".last_sessions.json";

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
    let index_file = sessions_dir.join(INDEX_FILE);
    let mut map = read_index(&index_file)?;
    map.insert(cwd_key(std::fs::canonicalize(cwd), cwd), session_id.to_string());
    write_atomic(&index_file, &encode_index(&map)?, 0o600)?;
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
