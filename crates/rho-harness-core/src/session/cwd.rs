use crate::error::{AppError, Result};
use std::path::Path;

pub fn record_session_for_cwd(sessions_dir: &Path, cwd: &Path, session_id: &str) -> Result<()> {
    std::fs::create_dir_all(sessions_dir)?;
    let index_file = sessions_dir.join(".last_sessions.json");
    let mut map: std::collections::BTreeMap<String, String> = if index_file.exists() {
        let content = std::fs::read_to_string(&index_file)?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        std::collections::BTreeMap::new()
    };
    let canonical_cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    map.insert(canonical_cwd.display().to_string(), session_id.to_string());
    let json = serde_json::to_string_pretty(&map).map_err(|e| AppError::Session(e.to_string()))?;
    std::fs::write(&index_file, json)?;
    Ok(())
}

pub fn last_session_for_cwd(sessions_dir: &Path, cwd: &Path) -> Result<Option<String>> {
    let index_file = sessions_dir.join(".last_sessions.json");
    if !index_file.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&index_file)?;
    let map: std::collections::BTreeMap<String, String> = serde_json::from_str(&content).unwrap_or_default();
    let canonical_cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    Ok(map.get(&canonical_cwd.display().to_string()).cloned())
}
