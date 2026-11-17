//! AuthStore disk serialization and deserialization.

use rho_harness_core::auth::StoredCredential;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::fs::write_atomic;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(untagged)]
enum RawStoredEntry {
    Structured(StoredCredential),
    LegacyString(String),
}

pub(super) fn load_credentials(path: &Path) -> Result<HashMap<String, StoredCredential>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let content =
        std::fs::read_to_string(path).map_err(|e| AppError::Auth(format!("Failed to read auth file: {e}")))?;
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
    if content.trim().is_empty() {
        return Ok(HashMap::new());
    }
    let raw_map: HashMap<String, RawStoredEntry> = serde_json::from_str(content)
        .map_err(|e| AppError::Auth(format!("Failed to parse auth file {}: {e}", path.display())))?;
    Ok(parse_credentials_map(raw_map))
}

pub(super) async fn load_credentials_async(path: &Path) -> Result<HashMap<String, StoredCredential>> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(HashMap::new());
    }
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| AppError::Auth(format!("Failed to read auth file: {e}")))?;
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
    if content.trim().is_empty() {
        return Ok(HashMap::new());
    }
    let raw_map: HashMap<String, RawStoredEntry> = serde_json::from_str(content)
        .map_err(|e| AppError::Auth(format!("Failed to parse auth file {}: {e}", path.display())))?;
    Ok(parse_credentials_map(raw_map))
}

fn parse_credentials_map(raw_map: HashMap<String, RawStoredEntry>) -> HashMap<String, StoredCredential> {
    let mut credentials = HashMap::new();
    for (k, entry) in raw_map {
        match entry {
            RawStoredEntry::Structured(mut c) => {
                if let StoredCredential::OAuth {
                    ref access_token,
                    ref mut account_id,
                    ..
                } = c
                    && account_id.is_none()
                    && k == "chatgpt"
                {
                    *account_id = crate::auth::oauth::extract_chatgpt_account_id(access_token);
                }
                credentials.insert(k, c);
            }
            RawStoredEntry::LegacyString(s) => {
                credentials.insert(k, StoredCredential::api_key(s));
            }
        }
    }
    credentials
}

fn encode_credentials(credentials: &HashMap<String, StoredCredential>) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(credentials).map_err(|e| AppError::Auth(format!("Failed to serialize auth store: {e}")))
}

fn write_error(path: &Path, error: std::io::Error) -> AppError {
    AppError::Auth(format!("Failed to write auth file {}: {error}", path.display()))
}

fn save_credentials(path: &Path, credentials: &HashMap<String, StoredCredential>) -> Result<()> {
    let json = encode_credentials(credentials)?;
    write_atomic(path, &json, 0o600).map_err(|e| write_error(path, e))
}

fn join_error(error: tokio::task::JoinError) -> AppError {
    AppError::Auth(format!("Auth file task failed: {error}"))
}

/// Exclusive advisory lock on `<auth file>.lock`, shared by every rho process; released on drop.
pub(super) struct AuthFileLock {
    _file: File,
}

fn lock_path(path: &Path) -> Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| AppError::Auth(format!("Auth file path '{}' has no file name", path.display())))?;
    let mut lock_name = name.to_os_string();
    lock_name.push(".lock");
    Ok(path.with_file_name(lock_name))
}

pub(super) fn lock_auth_file(path: &Path) -> Result<AuthFileLock> {
    let lock = lock_path(path)?;
    let lock_error = |e: std::io::Error| AppError::Auth(format!("Failed to lock auth file {}: {e}", lock.display()));
    if let Some(dir) = lock.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(lock_error)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock)
        .map_err(lock_error)?;
    file.lock().map_err(lock_error)?;
    Ok(AuthFileLock { _file: file })
}

pub(super) async fn lock_auth_file_async(path: &Path) -> Result<AuthFileLock> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || lock_auth_file(&path))
        .await
        .map_err(join_error)?
}

/// Rewrites only `provider`'s entry on top of the latest file contents. Caller must hold the lock.
fn write_entry_locked(path: &Path, provider: &str, entry: Option<&StoredCredential>) -> Result<()> {
    let mut credentials = load_credentials(path)?;
    match entry {
        Some(credential) => credentials.insert(provider.to_string(), credential.clone()),
        None => credentials.remove(provider),
    };
    save_credentials(path, &credentials)
}

pub(super) fn write_entry(path: &Path, provider: &str, entry: Option<&StoredCredential>) -> Result<()> {
    let _lock = lock_auth_file(path)?;
    write_entry_locked(path, provider, entry)
}

pub(super) async fn write_entry_async(path: &Path, provider: &str, entry: Option<StoredCredential>) -> Result<()> {
    let (path, provider) = (path.to_path_buf(), provider.to_string());
    tokio::task::spawn_blocking(move || write_entry(&path, &provider, entry.as_ref()))
        .await
        .map_err(join_error)?
}

pub(super) async fn write_entry_locked_async(
    _lock: &AuthFileLock,
    path: &Path,
    provider: &str,
    entry: StoredCredential,
) -> Result<()> {
    let (path, provider) = (path.to_path_buf(), provider.to_string());
    tokio::task::spawn_blocking(move || write_entry_locked(&path, &provider, Some(&entry)))
        .await
        .map_err(join_error)?
}
