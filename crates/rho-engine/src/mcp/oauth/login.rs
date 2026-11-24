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
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CLIENT_NAME: &str = "rho";
/// oh-my-pi's default callback port; clients registered through it expect `http://localhost:3000/callback`.
const PINNED_CLIENT_PORT: u16 = 3000;
pub(super) const ACCEPT_JSON: [(&str, &str); 1] = [("accept", "application/json")];

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
    scope: Option<&str>,
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
             the provider and set `oauth.clientId` (and `oauth.redirectUri`) for this server in your MCP config."
                .to_string(),
        )
    })?;
    let mut body = serde_json::json!({
        "client_name": CLIENT_NAME,
        "redirect_uris": [redirect],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    if let Some(scope) = scope.map(str::trim).filter(|s| !s.is_empty()) {
        body["scope"] = serde_json::json!(scope);
    }
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
    extra: Option<&'a BTreeMap<String, String>>,
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
    let present: BTreeSet<String> = url.query_pairs().map(|(k, _)| k.into_owned()).collect();
    let extra = params.extra.into_iter().flatten();
    url.query_pairs_mut()
        .extend_pairs(extra.filter(|(k, _)| !present.contains(*k)));
    let wants_offline = params
        .scope
        .is_some_and(|s| s.split_whitespace().any(|s| s == "offline_access"));
    if wants_offline && !url.query_pairs().any(|(k, _)| k == "prompt") {
        // OpenID Connect Core 11: offline_access is ignored unless prompt includes consent.
        url.query_pairs_mut().append_pair("prompt", "consent");
    }
    Ok(url.to_string())
}

fn redirect_target(config: Option<&McpOAuthConfig>) -> Result<(u16, Option<String>)> {
    if let Some(raw) = config.and_then(|c| c.redirect_uri.as_deref()) {
        let url =
            url::Url::parse(raw).map_err(|e| AppError::Auth(format!("Invalid oauth.redirectUri '{raw}': {e}")))?;
        if url.scheme() != "http" || !matches!(url.host_str(), Some("localhost" | "127.0.0.1")) {
            return Err(AppError::Auth(format!(
                "oauth.redirectUri '{raw}' must be an http://localhost or http://127.0.0.1 URL so rho can receive the callback"
            )));
        }
        return Ok((url.port_or_known_default().unwrap_or(80), Some(raw.to_string())));
    }
    let pinned_client = config.is_some_and(|c| c.client_id.is_some());
    let default_port = if pinned_client { PINNED_CLIENT_PORT } else { 0 };
    Ok((config.and_then(|c| c.redirect_port).unwrap_or(default_port), None))
}

async fn bind_redirect(config: Option<&McpOAuthConfig>) -> Result<(LoopbackServer, String)> {
    let (port, exact) = redirect_target(config)?;
    let loopback = LoopbackServer::bind_port(port).await?;
    let redirect = exact.unwrap_or_else(|| loopback.redirect_uri("/callback"));
    Ok((loopback, redirect))
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

    let (loopback, redirect) = bind_redirect(config).await?;
    let client = resolve_client(config, &auth_meta, &redirect, scope.as_deref()).await?;
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
        extra: config.map(|c| &c.authorization_params),
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
        &ACCEPT_JSON,
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
        server_url: Some(url.to_string()),
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

    #[test]
    fn redirect_target_prefers_exact_uri_then_port_then_pinned_default() {
        let cfg = |client: Option<&str>, port: Option<u16>, uri: Option<&str>| McpOAuthConfig {
            client_id: client.map(Into::into),
            redirect_port: port,
            redirect_uri: uri.map(Into::into),
            ..Default::default()
        };
        let uri = "http://localhost:19876/callback";
        let cases = [
            (None, (0, None)),
            (Some(cfg(None, None, None)), (0, None)),
            (Some(cfg(Some("c"), None, None)), (PINNED_CLIENT_PORT, None)),
            (Some(cfg(Some("c"), Some(8765), None)), (8765, None)),
            (
                Some(cfg(Some("c"), Some(8765), Some(uri))),
                (19876, Some(uri.to_string())),
            ),
            (
                Some(cfg(None, None, Some("http://127.0.0.1/cb"))),
                (80, Some("http://127.0.0.1/cb".to_string())),
            ),
        ];
        for (config, expected) in cases {
            assert_eq!(redirect_target(config.as_ref()).unwrap(), expected, "{config:?}");
        }
        for bad in ["https://localhost:1/cb", "http://example.com/cb", "not a url"] {
            assert!(redirect_target(Some(&cfg(None, None, Some(bad)))).is_err(), "{bad}");
        }
    }

    #[test]
    fn offline_access_scope_adds_consent_prompt_unless_one_is_configured() {
        let prompts = |scope: Option<&str>, extra: &[(&str, &str)]| {
            let extra: BTreeMap<String, String> = extra.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            let url = authorize_url(&AuthorizeUrl {
                endpoint: "https://auth.example.com/authorize",
                client_id: "cid",
                redirect: "http://localhost:3000/callback",
                challenge: "ch",
                state: "st",
                scope,
                resource: "https://mcp.example.com/mcp",
                extra: Some(&extra),
            })
            .unwrap();
            url::Url::parse(&url)
                .unwrap()
                .query_pairs()
                .filter(|(k, _)| k == "prompt")
                .map(|(_, v)| v.into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(prompts(Some("openid offline_access"), &[]), ["consent"]);
        assert_eq!(
            prompts(Some("offline_access"), &[("prompt", "select_account consent")]),
            ["select_account consent"]
        );
        assert!(prompts(Some("read"), &[]).is_empty());
        assert!(prompts(None, &[]).is_empty());
    }

    #[test]
    fn authorization_params_are_added_without_overriding_protocol_params() {
        let extra = BTreeMap::from([
            ("access_type".to_string(), "offline".to_string()),
            ("state".to_string(), "hijack".to_string()),
        ]);
        let url = authorize_url(&AuthorizeUrl {
            endpoint: "https://auth.example.com/authorize?tenant=t1",
            client_id: "cid",
            redirect: "http://localhost:3000/callback",
            challenge: "ch",
            state: "st",
            scope: None,
            resource: "https://mcp.example.com/mcp",
            extra: Some(&extra),
        })
        .unwrap();
        let pairs: Vec<(String, String)> = url::Url::parse(&url).unwrap().query_pairs().into_owned().collect();
        let values = |key: &str| {
            pairs
                .iter()
                .filter(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(values("access_type"), ["offline"]);
        assert_eq!(values("state"), ["st"]);
        assert_eq!(values("tenant"), ["t1"]);
    }
}
