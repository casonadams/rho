use crate::provider::error::error_message_or_excerpt;
use serde::de::DeserializeOwned;
use std::sync::LazyLock;
use std::time::Duration;

pub static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    crate::install_crypto_provider();
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .tcp_keepalive(Duration::from_secs(30))
        .build()
        .expect("failed to build shared HTTP client")
});

/// A header value from `value`; the failure names `what`, never the value, when it cannot be sent in a header.
pub(crate) fn header_value(
    value: &str,
    what: &str,
) -> Result<reqwest::header::HeaderValue, crate::provider::error::HttpFailure> {
    reqwest::header::HeaderValue::from_str(value).map_err(|_| {
        crate::provider::error::HttpFailure::Local(format!(
            "{what} contains characters that cannot be sent in an HTTP header; log in again or check the configured key"
        ))
    })
}

pub(crate) fn bearer(token: &str) -> Result<reqwest::header::HeaderValue, crate::provider::error::HttpFailure> {
    header_value(&format!("Bearer {token}"), "The access token")
}

pub async fn post_json_checked(
    url: &str,
    headers: reqwest::header::HeaderMap,
    body: &serde_json::Value,
) -> crate::provider::error::HttpResult {
    use crate::provider::error::HttpFailure;
    let response = CLIENT
        .post(url)
        .headers(headers)
        .json(body)
        .send()
        .await
        .map_err(|e| HttpFailure::Transport(e.to_string()))?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let retry_after = crate::provider::error::parse_retry_after(response.headers(), chrono::Utc::now());
    let body = response
        .text()
        .await
        .unwrap_or_else(|e| format!("(failed to read error body: {e})"));
    Err(HttpFailure::Status {
        status: status.as_u16(),
        body,
        retry_after,
    })
}

pub async fn send_json_if_ok<T: DeserializeOwned>(request: reqwest::RequestBuilder) -> Option<T> {
    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json().await.ok()
}

pub(crate) async fn get_json(
    request: reqwest::RequestBuilder,
    label: &str,
) -> Result<serde_json::Value, rho_harness_core::error::AppError> {
    let failed = |detail: String| rho_harness_core::error::AppError::Network(format!("{label} {detail}"));
    let response = request
        .send()
        .await
        .map_err(|e| failed(format!("request failed: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|e| format!("(failed to read error body: {e})"));
        return Err(failed(format!(
            "returned HTTP {status}: {}",
            error_message_or_excerpt(&body)
        )));
    }
    response
        .json()
        .await
        .map_err(|e| failed(format!("returned invalid JSON: {e}")))
}

pub(crate) fn friendly_oauth_error(name: &str, login: &str, status: Option<u16>, body: &str) -> String {
    let message = error_message_or_excerpt(body);
    match status {
        Some(401) => format!("{name} OAuth session expired or credentials are invalid. Run 'rho login {login}'."),
        Some(429) => format!("{name} rate limit or usage limit reached. Wait a bit and retry. Backend: {message}"),
        Some(403) => format!("{name} access denied. Backend: {message}"),
        Some(400) => format!("{name} request invalid. Backend: {message}"),
        Some(502 | 503) => format!("{name} service is temporarily unavailable. Wait a bit and retry."),
        Some(529) => format!("{name} API is overloaded. Wait a bit and retry."),
        Some(other) => format!("{name} API error ({other}): {message}"),
        None => {
            let prefix = format!("{name} request failed: ");
            let clean = message.strip_prefix(&prefix).unwrap_or(&message);
            format!("{prefix}{clean}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::serve_http;
    use test_case::test_case;

    #[test]
    fn unsendable_credentials_fail_locally_without_echoing_the_secret() {
        let secret = "sk-secret\nvalue";
        let failures = [
            bearer(secret).unwrap_err(),
            crate::claude::http::claude_headers(crate::claude::http::ClaudeAuth::Login, secret).unwrap_err(),
            crate::claude::http::claude_headers(crate::claude::http::ClaudeAuth::ApiKey, secret).unwrap_err(),
            crate::antigravity::client::http::antigravity_headers(secret).unwrap_err(),
        ];
        for failure in failures {
            assert!(
                matches!(failure, crate::provider::error::HttpFailure::Local(_)),
                "{failure:?}"
            );
            assert!(
                failure.body().contains("cannot be sent in an HTTP header"),
                "{failure:?}"
            );
            assert!(!failure.body().contains("sk-secret"), "{failure:?}");
        }
        assert_eq!(bearer("abc").unwrap(), "Bearer abc");
    }

    #[test_case("200 OK", r#"{"ok":true}"#, true; "success with json")]
    #[test_case("500 Internal Server Error", r#"{"ok":true}"#, false; "server error")]
    #[test_case("200 OK", "not json", false; "success without json")]
    #[tokio::test]
    async fn send_json_if_ok_returns_json_only_for_success_responses(
        status: &'static str,
        body: &str,
        expect_json: bool,
    ) {
        let url = serve_http(vec![(status, body.to_string())]).await;
        let value: Option<serde_json::Value> = send_json_if_ok(CLIENT.get(&url)).await;
        assert_eq!(
            value.map(|v| v["ok"].clone()),
            expect_json.then_some(serde_json::Value::Bool(true))
        );
    }
}
