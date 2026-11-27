//! OAuth 2.1 authorization for remote MCP servers (MCP spec 2025-11-25, "Authorization").

mod discovery;
mod login;
#[cfg(test)]
mod tests;

pub use login::{LoginRequest, login};

use crate::auth::token_request::{TokenResponse, post_form_json};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::fs::write_atomic;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::Mutex;

pub const TOKEN_FILE: &str = "mcp-auth.json";
const TOKEN_TIMEOUT: Duration = Duration::from_secs(30);
const EXPIRY_MARGIN_MS: i64 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpTokens {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<i64>,
    pub token_endpoint: String,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub resource: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_url: Option<String>,
}

impl McpTokens {
    /// Tokens saved before `server_url` existed are bound by their `resource` instead.
    fn is_bound_to(&self, url: &str) -> bool {
        let bound = self.server_url.as_deref().unwrap_or(&self.resource);
        match (discovery::canonical_resource(bound), discovery::canonical_resource(url)) {
            (Ok(bound), Ok(url)) => bound == url,
            _ => false,
        }
    }

    fn is_expired(&self) -> bool {
        self.expires_at_ms.is_some_and(|at| at <= crate::clock::unix_now_ms())
    }

    fn with_reply(&self, reply: TokenResponse) -> Self {
        Self {
            access_token: reply.access_token,
            refresh_token: reply.refresh_token.or_else(|| self.refresh_token.clone()),
            expires_at_ms: expires_at(reply.expires_in),
            ..self.clone()
        }
    }
}

fn expires_at(expires_in: Option<i64>) -> Option<i64> {
    expires_in.map(|secs| crate::clock::expires_at_ms(secs, EXPIRY_MARGIN_MS))
}

#[derive(Debug, Clone)]
pub struct McpTokenStore {
    path: PathBuf,
}

impl McpTokenStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn in_config_dir(config_dir: &Path) -> Self {
        Self::new(config_dir.join(TOKEN_FILE))
    }

    fn read_all(path: &Path) -> Result<BTreeMap<String, McpTokens>> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(AppError::Auth(format!("Failed to read {}: {e}", path.display()))),
        };
        if text.trim().is_empty() {
            return Ok(BTreeMap::new());
        }
        serde_json::from_str(&text).map_err(|e| AppError::Auth(format!("Failed to parse {}: {e}", path.display())))
    }

    fn update(path: &Path, server: &str, tokens: Option<McpTokens>) -> Result<bool> {
        let mut all = Self::read_all(path)?;
        let changed = match tokens {
            Some(tokens) => {
                all.insert(server.to_string(), tokens);
                true
            }
            None => all.remove(server).is_some(),
        };
        let json = serde_json::to_vec_pretty(&all).map_err(|e| AppError::Auth(e.to_string()))?;
        write_atomic(path, &json, 0o600)
            .map_err(|e| AppError::Auth(format!("Failed to write {}: {e}", path.display())))?;
        Ok(changed)
    }

    async fn blocking<T: Send + 'static>(&self, work: impl FnOnce(&Path) -> Result<T> + Send + 'static) -> Result<T> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || work(&path))
            .await
            .map_err(|e| AppError::Auth(format!("MCP token store task failed: {e}")))?
    }

    pub async fn load(&self, server: &str) -> Result<Option<McpTokens>> {
        let server = server.to_string();
        self.blocking(move |path| Ok(Self::read_all(path)?.remove(&server)))
            .await
    }

    pub async fn save(&self, server: &str, tokens: McpTokens) -> Result<()> {
        let server = server.to_string();
        self.blocking(move |path| Self::update(path, &server, Some(tokens)).map(|_| ()))
            .await
    }

    pub async fn remove(&self, server: &str) -> Result<bool> {
        let server = server.to_string();
        self.blocking(move |path| Self::update(path, &server, None)).await
    }

    pub fn forget(&self, server: &str) -> Result<bool> {
        Self::update(&self.path, server, None)
    }

    #[must_use]
    pub fn is_signed_in(&self, server: &str) -> bool {
        Self::read_all(&self.path).is_ok_and(|all| all.contains_key(server))
    }
}

async fn refresh_tokens(server: &str, tokens: &McpTokens) -> Result<McpTokens> {
    let refresh = tokens.refresh_token.as_deref().ok_or_else(|| {
        AppError::Auth(format!(
            "The token for MCP server '{server}' expired and cannot be refreshed. {}",
            login_hint(server)
        ))
    })?;
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh),
        ("client_id", tokens.client_id.as_str()),
        ("resource", tokens.resource.as_str()),
    ];
    if let Some(secret) = &tokens.client_secret {
        form.push(("client_secret", secret));
    }
    let reply: TokenResponse = post_form_json(
        &tokens.token_endpoint,
        &form,
        &login::ACCEPT_JSON,
        TOKEN_TIMEOUT,
        "MCP token refresh",
    )
    .await
    .map_err(|e| AppError::Auth(format!("{e}. {}", login_hint(server))))?;
    Ok(tokens.with_reply(reply))
}

#[must_use]
pub fn login_hint(server: &str) -> String {
    format!("Run `/mcp login {server}` (or `rho mcp login {server}`) to sign in.")
}

pub struct McpOAuth {
    server: String,
    url: String,
    store: McpTokenStore,
    tokens: Mutex<Option<Option<McpTokens>>>,
}

impl McpOAuth {
    #[must_use]
    pub fn new(server: impl Into<String>, url: impl Into<String>, store: McpTokenStore) -> Self {
        Self {
            server: server.into(),
            url: url.into(),
            store,
            tokens: Mutex::new(None),
        }
    }

    #[must_use]
    pub fn server(&self) -> &str {
        &self.server
    }

    pub(crate) async fn bearer(&self) -> Result<Option<String>> {
        let mut guard = self.tokens.lock().await;
        if guard.is_none() {
            *guard = Some(self.store.load(&self.server).await?);
        }
        let Some(Some(tokens)) = guard.as_ref() else {
            return Ok(None);
        };
        if !tokens.is_bound_to(&self.url) {
            return Err(AppError::Auth(format!(
                "The saved sign-in for MCP server '{}' was issued for a different URL than {}. {}",
                self.server,
                self.url,
                login_hint(&self.server)
            )));
        }
        if tokens.is_expired() && tokens.refresh_token.is_some() {
            let refreshed = self.refresh(tokens).await?;
            *guard = Some(Some(refreshed));
        }
        Ok(guard.as_ref().and_then(Option::as_ref).map(|t| t.access_token.clone()))
    }

    async fn refresh(&self, tokens: &McpTokens) -> Result<McpTokens> {
        let refreshed = refresh_tokens(&self.server, tokens).await.map_err(|e| {
            AppError::Auth(format!(
                "MCP server '{}' sign-in expired and could not be refreshed ({e}). {}",
                self.server,
                login_hint(&self.server)
            ))
        })?;
        self.store.save(&self.server, refreshed.clone()).await?;
        Ok(refreshed)
    }

    pub(crate) async fn refresh_after_rejection(&self, rejected: &str) -> Result<Option<String>> {
        let mut guard = self.tokens.lock().await;
        let Some(Some(tokens)) = guard.as_ref() else {
            return Ok(None);
        };
        if tokens.access_token != rejected {
            return Ok(Some(tokens.access_token.clone()));
        }
        if tokens.refresh_token.is_none() {
            return Ok(None);
        }
        let refreshed = self.refresh(tokens).await?;
        let access = refreshed.access_token.clone();
        *guard = Some(Some(refreshed));
        Ok(Some(access))
    }
}
