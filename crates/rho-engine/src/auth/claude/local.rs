use rho_harness_core::auth::StoredCredential;
use std::path::Path;

pub fn parse_claude_credentials_json(raw: &str) -> Option<(String, Option<String>, Option<i64>)> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let target = value.get("claudeAiOauth").unwrap_or(&value);

    let access_token = target
        .get("accessToken")
        .or_else(|| target.get("access_token"))
        .and_then(|v| v.as_str())?
        .trim();

    if access_token.is_empty() {
        return None;
    }

    let refresh_token = target
        .get("refreshToken")
        .or_else(|| target.get("refresh_token"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let expires_at_ms = parse_expires_at(target.get("expiresAt").or_else(|| target.get("expires_at")));
    Some((access_token.to_string(), refresh_token, expires_at_ms))
}

fn parse_expires_at(value: Option<&serde_json::Value>) -> Option<i64> {
    let val = value?;
    if let Some(num) = val.as_i64() {
        return Some(if num > 10_000_000_000 { num } else { num * 1000 });
    }
    if let Some(num_f) = val.as_f64() {
        let n = num_f as i64;
        return Some(if n > 10_000_000_000 { n } else { n * 1000 });
    }
    val.as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp_millis())
}

pub fn parse_claude_json_metadata(raw: &str) -> (Option<String>, Option<String>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return (None, None);
    };
    let Some(oauth) = value.get("oauthAccount") else {
        return (None, None);
    };
    let org_uuid = oauth
        .get("organizationUuid")
        .or_else(|| oauth.get("accountUuid"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let email = oauth.get("emailAddress").and_then(|v| v.as_str()).map(String::from);
    (org_uuid, email)
}

fn decode_keychain_output(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        return Some(trimmed.to_string());
    }
    if trimmed.len().is_multiple_of(2) && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        let bytes: Option<Vec<u8>> = (0..trimmed.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&trimmed[i..i + 2], 16).ok())
            .collect();
        if let Some(bytes) = bytes
            && let Ok(s) = String::from_utf8(bytes)
            && s.trim().starts_with('{')
        {
            return Some(s.trim().to_string());
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn read_keychain_credentials() -> Option<String> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    decode_keychain_output(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "macos"))]
fn read_keychain_credentials() -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
async fn read_keychain_credentials_async() -> Option<String> {
    let output = tokio::process::Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    decode_keychain_output(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "macos"))]
async fn read_keychain_credentials_async() -> Option<String> {
    None
}

fn enrich_metadata(mut cred: StoredCredential, config_path: Option<&Path>) -> StoredCredential {
    if let Some(cfg) = config_path
        && cfg.exists()
        && let Ok(raw_cfg) = std::fs::read_to_string(cfg)
    {
        let (id, email) = parse_claude_json_metadata(&raw_cfg);
        if let StoredCredential::OAuth {
            account_id,
            account_email,
            ..
        } = &mut cred
        {
            *account_id = id;
            *account_email = email;
        }
    }
    cred
}

async fn enrich_metadata_async(mut cred: StoredCredential, config_path: Option<&Path>) -> StoredCredential {
    if let Some(cfg) = config_path
        && tokio::fs::try_exists(cfg).await.unwrap_or(false)
        && let Ok(raw_cfg) = tokio::fs::read_to_string(cfg).await
    {
        let (id, email) = parse_claude_json_metadata(&raw_cfg);
        if let StoredCredential::OAuth {
            account_id,
            account_email,
            ..
        } = &mut cred
        {
            *account_id = id;
            *account_email = email;
        }
    }
    cred
}

pub fn detect_credentials_from_paths(creds_path: &Path, config_path: Option<&Path>) -> Option<StoredCredential> {
    let raw = std::fs::read_to_string(creds_path).ok()?;
    let (access, refresh, exp) = parse_claude_credentials_json(&raw)?;
    Some(enrich_metadata(
        StoredCredential::oauth(access, refresh, exp),
        config_path,
    ))
}

pub async fn detect_credentials_from_paths_async(
    creds_path: &Path,
    config_path: Option<&Path>,
) -> Option<StoredCredential> {
    let raw = tokio::fs::read_to_string(creds_path).await.ok()?;
    let (access, refresh, exp) = parse_claude_credentials_json(&raw)?;
    Some(enrich_metadata_async(StoredCredential::oauth(access, refresh, exp), config_path).await)
}

pub fn detect_local_claude_credentials() -> Option<StoredCredential> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(std::path::PathBuf::from)?;

    let creds_file = home.join(".claude").join(".credentials.json");
    let config_file = home.join(".claude.json");

    let file_cred = if creds_file.exists() {
        detect_credentials_from_paths(&creds_file, Some(&config_file))
    } else {
        None
    };

    if let Some(cred) = &file_cred
        && !cred.is_expired(60)
    {
        return Some(cred.clone());
    }

    if let Some(raw_kc) = read_keychain_credentials()
        && let Some((access, refresh, exp)) = parse_claude_credentials_json(&raw_kc)
    {
        let cred = StoredCredential::oauth(access, refresh, exp);
        return Some(enrich_metadata(cred, Some(&config_file)));
    }

    file_cred
}

pub async fn detect_local_claude_credentials_async() -> Option<StoredCredential> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(std::path::PathBuf::from)?;

    let creds_file = home.join(".claude").join(".credentials.json");
    let config_file = home.join(".claude.json");

    let file_cred = if tokio::fs::try_exists(&creds_file).await.unwrap_or(false) {
        detect_credentials_from_paths_async(&creds_file, Some(&config_file)).await
    } else {
        None
    };

    if let Some(cred) = &file_cred
        && !cred.is_expired(60)
    {
        return Some(cred.clone());
    }

    if let Some(raw_kc) = read_keychain_credentials_async().await
        && let Some((access, refresh, exp)) = parse_claude_credentials_json(&raw_kc)
    {
        let cred = StoredCredential::oauth(access, refresh, exp);
        return Some(enrich_metadata_async(cred, Some(&config_file)).await);
    }

    file_cred
}
