use crate::auth::token_request::send_for_json;
use crate::http::CLIENT;
use rho_harness_core::error::{AppError, Result};
use serde::Deserialize;
use std::time::Duration;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Challenge {
    pub resource_metadata: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct ResourceMetadata {
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub authorization_servers: Vec<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct AuthServerMetadata {
    #[serde(default)]
    pub issuer: Option<String>,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub registration_endpoint: Option<String>,
    #[serde(default)]
    pub code_challenge_methods_supported: Option<Vec<String>>,
}

/// Reads `resource_metadata` and `scope` from a `WWW-Authenticate: Bearer ...` challenge (RFC 9728 5.1, RFC 6750 3).
pub(crate) fn parse_challenge(header: &str) -> Challenge {
    let params = header
        .trim()
        .strip_prefix("Bearer")
        .or_else(|| header.trim().strip_prefix("bearer"))
        .unwrap_or(header);
    let mut challenge = Challenge::default();
    let mut rest = params.trim_start();
    while let Some((key, after_eq)) = rest.split_once('=') {
        let key = key.trim().trim_start_matches(',').trim();
        let (value, remainder) = take_param_value(after_eq.trim_start());
        match key {
            "resource_metadata" => challenge.resource_metadata = Some(value),
            "scope" => challenge.scope = Some(value),
            _ => {}
        }
        rest = remainder.trim_start().trim_start_matches(',');
    }
    challenge
}

fn take_param_value(input: &str) -> (String, &str) {
    if let Some(quoted) = input.strip_prefix('"') {
        let end = quoted.find('"').unwrap_or(quoted.len());
        let rest = quoted.get(end + 1..).unwrap_or("");
        return (quoted[..end].to_string(), rest);
    }
    let end = input.find(',').unwrap_or(input.len());
    (input[..end].trim().to_string(), &input[end..])
}

/// The RFC 8707 resource identifier: lowercase scheme and host, no fragment, no lone trailing slash.
pub(crate) fn canonical_resource(server_url: &str) -> Result<String> {
    let mut url = url::Url::parse(server_url)
        .map_err(|e| AppError::Auth(format!("Invalid MCP server URL '{server_url}': {e}")))?;
    url.set_fragment(None);
    let text = url.to_string();
    Ok(text.strip_suffix('/').map_or(text.clone(), str::to_string))
}

fn origin_and_path(raw: &str) -> Result<(String, String)> {
    let url = url::Url::parse(raw).map_err(|e| AppError::Auth(format!("Invalid URL '{raw}': {e}")))?;
    let origin = url.origin().ascii_serialization();
    let path = url.path().trim_end_matches('/').to_string();
    Ok((origin, path))
}

/// Protected resource metadata locations to try when the challenge names none: path-specific, then root.
pub(crate) fn resource_metadata_urls(server_url: &str) -> Result<Vec<String>> {
    let (origin, path) = origin_and_path(server_url)?;
    let root = format!("{origin}/.well-known/oauth-protected-resource");
    if path.is_empty() {
        return Ok(vec![root]);
    }
    Ok(vec![format!("{root}{path}"), root])
}

/// Authorization server metadata locations in the order the MCP authorization spec requires.
pub(crate) fn auth_server_metadata_urls(issuer: &str) -> Result<Vec<String>> {
    let (origin, path) = origin_and_path(issuer)?;
    if path.is_empty() {
        return Ok(vec![
            format!("{origin}/.well-known/oauth-authorization-server"),
            format!("{origin}/.well-known/openid-configuration"),
        ]);
    }
    Ok(vec![
        format!("{origin}/.well-known/oauth-authorization-server{path}"),
        format!("{origin}/.well-known/openid-configuration{path}"),
        format!("{origin}{path}/.well-known/openid-configuration"),
    ])
}

/// RFC 8414 3.3: the metadata `issuer` must match the issuer it was fetched for. A trailing slash is ignored
/// because providers such as Google list `https://accounts.google.com/` but publish it without one.
pub(crate) fn check_issuer(expected: &str, published: Option<&str>) -> Result<()> {
    let Some(published) = published else {
        return Ok(());
    };
    if published.trim_end_matches('/') == expected.trim_end_matches('/') {
        return Ok(());
    }
    Err(AppError::Auth(format!(
        "Authorization server metadata names issuer '{published}', expected '{expected}'; refusing to sign in"
    )))
}

async fn first_json<T: serde::de::DeserializeOwned>(urls: &[String], what: &str) -> Result<T> {
    let mut failures = Vec::new();
    for url in urls {
        match send_for_json::<T>(CLIENT.get(url).timeout(DISCOVERY_TIMEOUT), what).await {
            Ok(value) => return Ok(value),
            Err(e) => failures.push(format!("{url}: {e}")),
        }
    }
    Err(AppError::Auth(format!(
        "Could not fetch {what}:\n  {}",
        failures.join("\n  ")
    )))
}

pub(crate) async fn fetch_resource_metadata(server_url: &str, challenge: &Challenge) -> Result<ResourceMetadata> {
    let urls = match &challenge.resource_metadata {
        Some(url) => vec![url.clone()],
        None => resource_metadata_urls(server_url)?,
    };
    let metadata: ResourceMetadata = first_json(&urls, "protected resource metadata").await?;
    if metadata.authorization_servers.is_empty() {
        return Err(AppError::Auth(
            "Protected resource metadata lists no authorization_servers".to_string(),
        ));
    }
    Ok(metadata)
}

pub(crate) async fn fetch_auth_server_metadata(issuer: &str) -> Result<AuthServerMetadata> {
    let metadata: AuthServerMetadata =
        first_json(&auth_server_metadata_urls(issuer)?, "authorization server metadata").await?;
    check_issuer(issuer, metadata.issuer.as_deref())?;
    let methods = metadata.code_challenge_methods_supported.as_deref();
    if methods.is_some_and(|m| !m.iter().any(|m| m == "S256")) {
        return Err(AppError::Auth(
            "Authorization server does not support PKCE S256, which MCP requires".to_string(),
        ));
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case(
        r#"Bearer resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource", scope="files:read files:write""#,
        Some("https://mcp.example.com/.well-known/oauth-protected-resource"),
        Some("files:read files:write");
        "metadata and scope"
    )]
    #[test_case(r#"Bearer error="invalid_token""#, None, None; "error only")]
    #[test_case(
        r#"Bearer realm="x",scope=read,resource_metadata="https://a/b""#,
        Some("https://a/b"),
        Some("read");
        "unquoted scope without spaces"
    )]
    #[test_case("", None, None; "empty header")]
    fn parses_bearer_challenges(header: &str, metadata: Option<&str>, scope: Option<&str>) {
        let challenge = parse_challenge(header);
        assert_eq!(challenge.resource_metadata.as_deref(), metadata);
        assert_eq!(challenge.scope.as_deref(), scope);
    }

    #[test]
    fn canonical_resource_normalizes_case_slash_and_fragment() {
        let cases = [
            ("https://MCP.Example.com/", "https://mcp.example.com"),
            ("https://mcp.example.com/mcp#frag", "https://mcp.example.com/mcp"),
            (
                "HTTPS://mcp.example.com:8443/server/mcp",
                "https://mcp.example.com:8443/server/mcp",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(canonical_resource(input).unwrap(), expected, "{input}");
        }
        assert!(canonical_resource("mcp.example.com").is_err());
    }

    #[test]
    fn issuer_must_match_ignoring_a_trailing_slash() {
        assert!(check_issuer("https://accounts.google.com/", Some("https://accounts.google.com")).is_ok());
        assert!(check_issuer("https://auth.example.com", Some("https://auth.example.com")).is_ok());
        assert!(check_issuer("https://auth.example.com", None).is_ok());
        let err = check_issuer("https://auth.example.com", Some("https://evil.example.com")).unwrap_err();
        assert!(err.to_string().contains("evil.example.com"), "{err}");
        assert!(check_issuer("https://auth.example.com/t1", Some("https://auth.example.com/t2")).is_err());
    }

    #[test]
    fn well_known_urls_follow_the_spec_order() {
        assert_eq!(
            resource_metadata_urls("https://example.com/public/mcp").unwrap(),
            [
                "https://example.com/.well-known/oauth-protected-resource/public/mcp",
                "https://example.com/.well-known/oauth-protected-resource",
            ]
        );
        assert_eq!(
            resource_metadata_urls("https://example.com").unwrap(),
            ["https://example.com/.well-known/oauth-protected-resource"]
        );
        assert_eq!(
            auth_server_metadata_urls("https://auth.example.com/tenant1").unwrap(),
            [
                "https://auth.example.com/.well-known/oauth-authorization-server/tenant1",
                "https://auth.example.com/.well-known/openid-configuration/tenant1",
                "https://auth.example.com/tenant1/.well-known/openid-configuration",
            ]
        );
        assert_eq!(
            auth_server_metadata_urls("https://auth.example.com/").unwrap(),
            [
                "https://auth.example.com/.well-known/oauth-authorization-server",
                "https://auth.example.com/.well-known/openid-configuration",
            ]
        );
    }
}
