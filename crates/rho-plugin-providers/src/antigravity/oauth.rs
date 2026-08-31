use super::client::HTTP_CLIENT;
use super::types::AntigravityTokens;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rho_core::error::{AppError, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub const REDIRECT_URI: &str = "http://localhost:51121/oauth-callback";
pub const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo?alt=json";
pub const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

pub const SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/aicode",
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
];

const DEFAULT_CLIENT_ID_B64: &str =
    "MTA3MTAwNjA2MDU5MS10bWhzc2luMmgyMWxjcmUyMzV2dG9sb2poNGc0MDNlcC5hcHBzLmdvb2dsZXVzZXJjb250ZW50LmNvbQ==";
const DEFAULT_CLIENT_SECRET_B64: &str = "R09DU1BYLUs1OEZXUjQ4NkxkTEoxbUxCOHNYQzR6NnFEQWY=";

pub fn client_id() -> String {
    if let Ok(id) = std::env::var("ANTIGRAVITY_CLIENT_ID")
        && !id.trim().is_empty()
    {
        return id.trim().to_string();
    }
    String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(DEFAULT_CLIENT_ID_B64)
            .unwrap_or_default(),
    )
    .unwrap_or_default()
}

pub fn client_secret() -> String {
    if let Ok(sec) = std::env::var("ANTIGRAVITY_CLIENT_SECRET")
        && !sec.trim().is_empty()
    {
        return sec.trim().to_string();
    }
    String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(DEFAULT_CLIENT_SECRET_B64)
            .unwrap_or_default(),
    )
    .unwrap_or_default()
}

pub fn generate_pkce() -> (String, String) {
    let mut random = [0u8; 32];
    for b in &mut random {
        *b = (fastrand::u32(..) & 0xFF) as u8;
    }
    let verifier = URL_SAFE_NO_PAD.encode(random);
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());
    (verifier, challenge)
}

pub fn generate_state() -> String {
    let mut random = [0u8; 24];
    for b in &mut random {
        *b = (fastrand::u32(..) & 0xFF) as u8;
    }
    URL_SAFE_NO_PAD.encode(random)
}

pub fn build_auth_url(challenge: &str, state: &str) -> String {
    let scope_str = SCOPES.join(" ");
    format!(
        "{AUTH_URL}?client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}&access_type=offline&prompt=consent",
        urlencoding(&client_id()),
        urlencoding(REDIRECT_URI),
        urlencoding(&scope_str)
    )
}

fn urlencoding(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

pub async fn start_callback_listener(expected_state: &str) -> Result<String> {
    let listener = match TcpListener::bind("127.0.0.1:51121").await {
        Ok(l) => l,
        Err(e) => {
            return Err(AppError::Auth(format!(
                "Port 51121 is already in use or unavailable: {e}"
            )));
        }
    };

    let accept_future = async {
        loop {
            let (mut stream, _) = listener.accept().await.map_err(|e| AppError::Auth(e.to_string()))?;
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).await.map_err(|e| AppError::Auth(e.to_string()))?;
            let request = String::from_utf8_lossy(&buf[..n]);
            let first_line = request.lines().next().unwrap_or_default();
            if first_line.starts_with("GET /oauth-callback") {
                let query = first_line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|path| path.split_once('?'))
                    .map(|(_, q)| q)
                    .unwrap_or_default();
                let params = parse_query(query);
                if let Some(error) = params.get("error") {
                    let response = format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nAuthentication error: {error}"
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    return Err(AppError::Auth(format!("OAuth provider error: {error}")));
                }
                let code = params.get("code").cloned();
                let state = params.get("state").cloned();
                if state.as_deref() != Some(expected_state) {
                    let response = "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nState mismatch";
                    let _ = stream.write_all(response.as_bytes()).await;
                    return Err(AppError::Auth("OAuth state parameter mismatch".to_string()));
                }
                if let Some(auth_code) = code {
                    let body = "<html><body style='font-family:sans-serif;text-align:center;padding-top:40px;'><h2>Google Antigravity authentication successful</h2><p>You can close this tab and return to rho.</p></body></html>";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    return Ok(auth_code);
                }
            }
        }
    };

    match tokio::time::timeout(CALLBACK_TIMEOUT, accept_future).await {
        Ok(res) => res,
        Err(_) => Err(AppError::Auth(
            "OAuth callback timed out waiting for Google sign-in".to_string(),
        )),
    }
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

pub async fn exchange_code_for_tokens(code: &str, code_verifier: &str) -> Result<AntigravityTokens> {
    let params = [
        ("client_id", client_id()),
        ("client_secret", client_secret()),
        ("code", code.to_string()),
        ("grant_type", "authorization_code".to_string()),
        ("redirect_uri", REDIRECT_URI.to_string()),
        ("code_verifier", code_verifier.to_string()),
    ];

    let response = HTTP_CLIENT
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to connect to Google OAuth: {e}")))?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!("Google token exchange failed: {text}")));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Invalid JSON from Google OAuth: {e}")))?;

    let access_token = json["access_token"]
        .as_str()
        .ok_or_else(|| AppError::Auth("No access_token received from Google".to_string()))?
        .to_string();
    let refresh_token = json["refresh_token"]
        .as_str()
        .ok_or_else(|| AppError::Auth("No refresh_token received from Google".to_string()))?
        .to_string();
    let expires_in = json["expires_in"].as_i64().unwrap_or(3600);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let expires_at = now + expires_in - 300;

    let email = fetch_user_email(&access_token).await.ok();
    let project_id = super::client::discover_project_id(&access_token)
        .await
        .or_else(|| email.as_deref().map(super::client::stable_project_id));

    Ok(AntigravityTokens {
        access_token,
        refresh_token,
        expires_at,
        project_id,
        email,
    })
}

pub async fn refresh_access_token(tokens: &AntigravityTokens) -> Result<AntigravityTokens> {
    let params = [
        ("client_id", client_id()),
        ("client_secret", client_secret()),
        ("refresh_token", tokens.refresh_token.clone()),
        ("grant_type", "refresh_token".to_string()),
    ];

    let response = HTTP_CLIENT
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to connect to Google OAuth: {e}")))?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!("Antigravity token refresh failed: {text}")));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Invalid JSON from token refresh: {e}")))?;

    let access_token = json["access_token"]
        .as_str()
        .ok_or_else(|| AppError::Auth("No access_token in refresh response".to_string()))?
        .to_string();
    let refresh_token = json["refresh_token"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| tokens.refresh_token.clone());
    let expires_in = json["expires_in"].as_i64().unwrap_or(3600);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let expires_at = now + expires_in - 300;

    Ok(AntigravityTokens {
        access_token,
        refresh_token,
        expires_at,
        project_id: tokens.project_id.clone(),
        email: tokens.email.clone(),
    })
}

pub async fn fetch_user_email(access_token: &str) -> Result<String> {
    let response = HTTP_CLIENT
        .get(USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| AppError::Auth(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::Auth("Failed to fetch user email".to_string()));
    }
    let json: serde_json::Value = response.json().await.map_err(|e| AppError::Auth(e.to_string()))?;
    json["email"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| AppError::Auth("Email not found in userinfo".to_string()))
}

pub fn auth_file_path(token_dir: &Path) -> PathBuf {
    token_dir.join("auth.json")
}

pub fn load_saved_tokens(token_dir: &Path) -> Result<Option<AntigravityTokens>> {
    let path = auth_file_path(token_dir);
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        let tokens: AntigravityTokens = serde_json::from_str(&content).map_err(|e| AppError::Auth(e.to_string()))?;
        return Ok(Some(tokens));
    }

    // Pi interoperability: fallback to ~/.pi/agent/auth.json if present.
    if let Some(home) = std::env::var_os("HOME") {
        let pi_auth = PathBuf::from(home).join(".pi/agent/auth.json");
        if pi_auth.exists()
            && let Ok(content) = std::fs::read_to_string(&pi_auth)
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&content)
            && let Some(antigravity_obj) = val.get("antigravity")
            && let Ok(tokens) = serde_json::from_value::<AntigravityTokens>(antigravity_obj.clone())
        {
            let _ = save_tokens(token_dir, &tokens);
            return Ok(Some(tokens));
        }
    }

    Ok(None)
}

pub fn save_tokens(token_dir: &Path, tokens: &AntigravityTokens) -> Result<()> {
    std::fs::create_dir_all(token_dir)?;
    let path = auth_file_path(token_dir);
    let json = serde_json::to_string_pretty(tokens).map_err(|e| AppError::Auth(e.to_string()))?;
    std::fs::write(&path, json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
