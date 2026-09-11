use rho_harness_core::error::{AppError, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ProtectedResourceMetadata {
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub authorization_servers: Vec<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthorizationServerMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub registration_endpoint: Option<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
}

fn extract_param_value<'a>(header: &'a str, param_name: &str) -> Option<&'a str> {
    let prefix = format!("{param_name}=");
    for part in header.split(',') {
        let trimmed = part.trim().strip_prefix("Bearer ").unwrap_or(part.trim()).trim();
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            let clean = rest.trim_matches('"').trim();
            if !clean.is_empty() {
                return Some(clean);
            }
        }
    }
    None
}

pub fn extract_resource_metadata_url(www_authenticate: &str) -> Option<String> {
    extract_param_value(www_authenticate, "resource_metadata").map(|s| s.to_string())
}

pub fn extract_required_scope(www_authenticate: &str) -> Option<String> {
    extract_param_value(www_authenticate, "scope").map(|s| s.to_string())
}

pub async fn fetch_protected_resource_metadata(
    client: &reqwest::Client,
    metadata_url: &str,
) -> Result<ProtectedResourceMetadata> {
    let res = client
        .get(metadata_url)
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to fetch protected resource metadata: {e}")))?;

    if !res.status().is_success() {
        return Err(AppError::Auth(format!(
            "Protected resource metadata returned status {}",
            res.status()
        )));
    }

    res.json::<ProtectedResourceMetadata>()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse protected resource metadata: {e}")))
}

pub async fn fetch_auth_server_metadata(
    client: &reqwest::Client,
    issuer_or_url: &str,
) -> Result<AuthorizationServerMetadata> {
    let base = issuer_or_url.trim_end_matches('/');
    let endpoints = [
        format!("{base}/.well-known/oauth-authorization-server"),
        format!("{base}/.well-known/openid-configuration"),
    ];

    for endpoint in &endpoints {
        if let Ok(res) = client.get(endpoint).send().await
            && res.status().is_success()
            && let Ok(meta) = res.json::<AuthorizationServerMetadata>().await
        {
            return Ok(meta);
        }
    }

    Err(AppError::Auth(format!(
        "Failed to discover authorization server metadata for '{issuer_or_url}'"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_resource_metadata_url_and_scope() {
        let header = r#"Bearer resource_metadata="https://example.com/.well-known/oauth-protected-resource", scope="tools:call files:read""#;
        assert_eq!(
            extract_resource_metadata_url(header),
            Some("https://example.com/.well-known/oauth-protected-resource".to_string())
        );
        assert_eq!(
            extract_required_scope(header),
            Some("tools:call files:read".to_string())
        );
    }

    #[test]
    fn test_extract_missing_url() {
        let header = "Bearer error=\"invalid_token\"";
        assert_eq!(extract_resource_metadata_url(header), None);
        assert_eq!(extract_required_scope(header), None);
    }
}
