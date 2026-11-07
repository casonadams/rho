//! Shared send/status-check/decode path for OAuth token endpoints.

use crate::http::CLIENT;
use rho_harness_core::error::{AppError, Result};
use serde::de::DeserializeOwned;
use std::time::Duration;

pub(crate) async fn send_checked(request: reqwest::RequestBuilder, operation: &str) -> Result<reqwest::Response> {
    let res = request
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("{operation} request failed: {e}")))?;
    if res.status().is_success() {
        return Ok(res);
    }
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    Err(AppError::Auth(format!("{operation} failed ({status}): {body}")))
}

pub(crate) async fn send_for_json<T: DeserializeOwned>(request: reqwest::RequestBuilder, operation: &str) -> Result<T> {
    send_checked(request, operation)
        .await?
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse {operation} response: {e}")))
}

pub(crate) async fn post_form_json<T: DeserializeOwned>(
    url: &str,
    form: &[(&str, &str)],
    headers: &[(&str, &str)],
    timeout: Duration,
    operation: &str,
) -> Result<T> {
    let mut request = CLIENT.post(url).timeout(timeout).form(form);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    send_for_json(request, operation).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::serve_http;
    use serde_json::Value;

    fn free_port_url() -> String {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        format!("http://127.0.0.1:{port}")
    }

    #[tokio::test]
    async fn send_checked_passes_success_and_reports_failed_status_with_body() {
        let ok = serve_http(vec![("200 OK", "{}".to_string())]).await;
        assert!(
            send_checked(CLIENT.get(&ok), "Probe")
                .await
                .unwrap()
                .status()
                .is_success()
        );

        let down = serve_http(vec![("503 Service Unavailable", "down".to_string())]).await;
        let err = send_checked(CLIENT.get(&down), "Probe").await.unwrap_err();
        assert!(
            err.to_string().contains("Probe failed (503 Service Unavailable): down"),
            "{err}"
        );

        let err = send_checked(CLIENT.get(free_port_url()), "Probe").await.unwrap_err();
        assert!(err.to_string().contains("Probe request failed"), "{err}");
    }

    #[tokio::test]
    async fn send_for_json_decodes_success_and_rejects_malformed_body() {
        let ok = serve_http(vec![("200 OK", r#"{"ok":true}"#.to_string())]).await;
        let value: Value = send_for_json(CLIENT.get(&ok), "Probe").await.unwrap();
        assert_eq!(value["ok"], true);

        let malformed = serve_http(vec![("200 OK", "not json".to_string())]).await;
        let err = send_for_json::<Value>(CLIENT.get(&malformed), "Probe")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Failed to parse Probe response"), "{err}");
    }

    #[tokio::test]
    async fn post_form_json_posts_form_and_decodes_reply() {
        let url = serve_http(vec![("200 OK", r#"{"access_token":"tok"}"#.to_string())]).await;
        let value: Value = post_form_json(
            &url,
            &[("grant_type", "refresh_token")],
            &[("User-Agent", "rho-test")],
            Duration::from_secs(5),
            "Refresh",
        )
        .await
        .unwrap();
        assert_eq!(value["access_token"], "tok");

        let url = serve_http(vec![("400 Bad Request", r#"{"error":"invalid_grant"}"#.to_string())]).await;
        let err = post_form_json::<Value>(&url, &[], &[], Duration::from_secs(5), "Refresh")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Refresh failed (400 Bad Request)"), "{err}");
    }
}
