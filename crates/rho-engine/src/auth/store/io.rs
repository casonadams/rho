//! AuthStore disk serialization and deserialization.

use rho_harness_core::auth::StoredCredential;
use rho_harness_core::error::{AppError, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
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
    let raw_map: HashMap<String, RawStoredEntry> = serde_json::from_str(&content).unwrap_or_default();
    Ok(parse_credentials_map(raw_map))
}

pub(super) async fn load_credentials_async(path: &Path) -> Result<HashMap<String, StoredCredential>> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(HashMap::new());
    }
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| AppError::Auth(format!("Failed to read auth file: {e}")))?;
    let raw_map: HashMap<String, RawStoredEntry> = serde_json::from_str(&content).unwrap_or_default();
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

pub(super) fn save_credentials(path: &Path, credentials: &HashMap<String, StoredCredential>) -> Result<()> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(credentials)
        .map_err(|e| AppError::Auth(format!("Failed to serialize auth store: {e}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(json.as_bytes())?;
    }

    #[cfg(not(unix))]
    {
        let mut file = OpenOptions::new().write(true).create(true).truncate(true).open(path)?;
        file.write_all(json.as_bytes())?;
    }

    Ok(())
}

pub(super) async fn save_credentials_async(path: &Path, credentials: &HashMap<String, StoredCredential>) -> Result<()> {
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let json = serde_json::to_string_pretty(credentials)
        .map_err(|e| AppError::Auth(format!("Failed to serialize auth store: {e}")))?;

    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let mut file = options.open(path).await?;
    tokio::io::AsyncWriteExt::write_all(&mut file, json.as_bytes()).await?;
    Ok(())
}
