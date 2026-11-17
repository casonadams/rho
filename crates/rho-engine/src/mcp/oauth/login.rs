use super::discovery::{
    AuthServerMetadata, Challenge, canonical_resource, fetch_auth_server_metadata, fetch_resource_metadata,
    parse_challenge,
};
use super::{McpTokenStore, McpTokens, expires_at};
use crate::auth::loopback::LoopbackServer;
use crate::auth::oauth::validate_callback;
use crate::auth::pkce::{PkceChallenge, generate_state};
use crate::auth::token_request::{TokenResponse, post_form_json, send_for_json};
use crate::http::CLIENT;
use crate::mcp::transport::http::MCP_PROTOCOL_VERSION_HEADER;
use rho_harness_core::auth::OAuthLoginCallbacks;
use rho_harness_core::config::McpOAuthConfig;
use rho_harness_core::error::{AppError, Result};
use serde::Deserialize;
use std::time::Duration;

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CLIENT_NAME: &str = "rho";

pub struct LoginRequest<'a> {
    pub server: &'a str,
    pub url: &'a str,
    pub config: Option<&'a McpOAuthConfig>,
}

struct Client {
    id: String,
    secret: Option<String>,
}

#[derive(Deserialize)]
struct Registration {
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
}

async fn probe(url: &str) -> Result<Challenge> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "method": "initialize",
        "params": {
            "protocolVersion": MCP_PROTOCOL_VERSION_HEADER,
            "capabilities": {},
            "clientInfo": { "name": CLIENT_NAME, "version": env!("CARGO_PKG_VERSION") },
        },
    });
    let res = CLIENT
        .post(url)
        .timeout(REQUEST_TIMEOUT)
        .header(reqwest::header::ACCEPT, "application/json, text/event-stream")
        .header("mcp-protocol-version", MCP_PROTOCOL_VERSION_HEADER)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Could not reach MCP server '{url}': {e}")))?;
    if res.status() != reqwest::StatusCode::UNAUTHORIZED {
        return Ok(Challenge::default());
    }
    let header = res
        .headers()
        .get(reqwest::header::WWW_AUTHENTICATE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    Ok(parse_challenge(header))
}

fn choose_scope(config: Option<&McpOAuthConfig>, challenge: &Challenge, supported: &[String]) -> Option<String> {
    let configured = config.map(|c| c.scopes.join(" ")).filter(|s| !s.is_empty());
    configured
        .or_else(|| challenge.scope.clone())
        .or_else(|| (!supported.is_empty()).then(|| supported.join(" ")))
}

async fn resolve_client(
    config: Option<&McpOAuthConfig>,
    metadata: &AuthServerMetadata,
    redirect: &str,
) -> Result<Client> {
    if let Some(id) = config.and_then(|c| c.client_id.clone()) {
        return Ok(Client {
            id,
            secret: config.and_then(|c| c.client_secret.clone()),
        });
    }
    let endpoint = metadata.registration_endpoint.as_deref().ok_or_else(|| {
        AppError::Auth(
            "The authorization server does not support dynamic client registration. Register an OAuth app with \
             the provider and set `oauth.clientId` (and `oauth.redirectPort`) for this server in your MCP config."
                .to_string(),
        )
    })?;
    let body = serde_json::json!({
        "client_name": CLIENT_NAME,
        "redirect_uris": [redirect],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    let request = CLIENT.post(endpoint).timeout(REQUEST_TIMEOUT).json(&body);
    let registration: Registration = send_for_json(request, "MCP client registration").await?;
    Ok(Client {
        id: registration.client_id,
        secret: registration.client_secret,
    })
}

struct AuthorizeUrl<'a> {
    endpoint: &'a str,
    client_id: &'a str,
    redirect: &'a str,
    challenge: &'a str,
    state: &'a str,
    scope: Option<&'a str>,
    resource: &'a str,
}

fn authorize_url(params: &AuthorizeUrl<'_>) -> Result<String> {
    let mut url = url::Url::parse(params.endpoint)
        .map_err(|e| AppError::Auth(format!("Invalid authorization endpoint '{}': {e}", params.endpoint)))?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", params.client_id)
            .append_pair("redirect_uri", params.redirect)
            .append_pair("code_challenge", params.challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", params.state)
            .append_pair("resource", params.resource);
        if let Some(scope) = params.scope {
            query.append_pair("scope", scope);
        }
    }
    Ok(url.to_string())
}

pub async fn login(
    request: LoginRequest<'_>,
    store: &McpTokenStore,
    callbacks: &dyn OAuthLoginCallbacks,
) -> Result<()> {
    let LoginRequest { server, url, config } = request;
    callbacks
        .on_progress(&format!("Discovering how to authorize MCP server '{server}'..."))
        .await?;
    let challenge = probe(url).await?;
    let resource_meta = fetch_resource_metadata(url, &challenge).await?;
    let auth_meta = fetch_auth_server_metadata(&resource_meta.authorization_servers[0]).await?;
    let resource = match &resource_meta.resource {
        Some(resource) => resource.clone(),
        None => canonical_resource(url)?,
    };
    let scope = choose_scope(config, &challenge, &resource_meta.scopes_supported);

    let loopback = LoopbackServer::bind_port(config.and_then(|c| c.redirect_port).unwrap_or(0)).await?;
    let redirect = loopback.redirect_uri("/callback");
    let client = resolve_client(config, &auth_meta, &redirect).await?;
    let pkce = PkceChallenge::generate();
    let state = generate_state();
    let auth_url = authorize_url(&AuthorizeUrl {
        endpoint: &auth_meta.authorization_endpoint,
        client_id: &client.id,
        redirect: &redirect,
        challenge: &pkce.challenge,
        state: &state,
        scope: scope.as_deref(),
        resource: &resource,
    })?;
    callbacks
        .on_auth_url(&auth_url, Some(&format!("Authorize rho to use MCP server '{server}'")))
        .await?;
    callbacks
        .on_progress("Waiting for authorization in your browser...")
        .await?;
    let code = validate_callback(loopback.wait_for_callback(CALLBACK_TIMEOUT).await?, &state)?;

    callbacks
        .on_progress("Exchanging authorization code for tokens...")
        .await?;
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("client_id", client.id.as_str()),
        ("code_verifier", pkce.verifier.as_str()),
        ("resource", resource.as_str()),
    ];
    if let Some(secret) = &client.secret {
        form.push(("client_secret", secret));
    }
    let reply: TokenResponse = post_form_json(
        &auth_meta.token_endpoint,
        &form,
        &[],
        REQUEST_TIMEOUT,
        "MCP token exchange",
    )
    .await?;
    let tokens = McpTokens {
        access_token: reply.access_token,
        refresh_token: reply.refresh_token,
        expires_at_ms: expires_at(reply.expires_in),
        token_endpoint: auth_meta.token_endpoint,
        client_id: client.id,
        client_secret: client.secret,
        resource,
    };
    store.save(server, tokens).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_prefers_config_then_challenge_then_supported() {
        let config = McpOAuthConfig {
            scopes: vec!["a".into(), "b".into()],
            ..Default::default()
        };
        let challenge = Challenge {
            resource_metadata: None,
            scope: Some("files:read".into()),
        };
        let supported = vec!["x".to_string(), "y".to_string()];
        assert_eq!(
            choose_scope(Some(&config), &challenge, &supported).as_deref(),
            Some("a b")
        );
        assert_eq!(
            choose_scope(None, &challenge, &supported).as_deref(),
            Some("files:read")
        );
        assert_eq!(
            choose_scope(None, &Challenge::default(), &supported).as_deref(),
            Some("x y")
        );
        assert_eq!(choose_scope(None, &Challenge::default(), &[]), None);
    }
}
