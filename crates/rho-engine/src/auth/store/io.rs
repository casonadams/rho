//! AuthStore disk serialization and deserialization.

use rho_harness_core::auth::StoredCredential;
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::fs::{write_atomic, write_atomic_async};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

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

pub(super) fn save_credentials(path: &Path, credentials: &HashMap<String, StoredCredential>) -> Result<()> {
    let json = encode_credentials(credentials)?;
    write_atomic(path, &json, 0o600).map_err(|e| write_error(path, e))
}

pub(super) async fn save_credentials_async(path: &Path, credentials: &HashMap<String, StoredCredential>) -> Result<()> {
    let json = encode_credentials(credentials)?;
    write_atomic_async(path, json, 0o600)
        .await
        .map_err(|e| write_error(path, e))
}
