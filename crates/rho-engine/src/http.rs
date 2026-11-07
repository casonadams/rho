use std::sync::LazyLock;
use std::time::Duration;

pub static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    crate::install_crypto_provider();
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("failed to build shared HTTP client")
});

pub async fn post_json_checked(
    url: &str,
    headers: reqwest::header::HeaderMap,
    body: &serde_json::Value,
    timeout: Option<Duration>,
) -> Result<reqwest::Response, (Option<u16>, String)> {
    let mut request = CLIENT.post(url).headers(headers).json(body);
    if let Some(timeout) = timeout {
        request = request.timeout(timeout);
    }
    let response = request.send().await.map_err(|e| (None, e.to_string()))?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let text = response.text().await.unwrap_or_default();
    Err((Some(status.as_u16()), text))
}

pub async fn send_json_if_ok(request: reqwest::RequestBuilder) -> Option<serde_json::Value> {
    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json().await.ok()
}

pub(crate) fn friendly_oauth_error(name: &str, login: &str, status: Option<u16>, body: &str) -> String {
    let message = backend_message(body);
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

fn backend_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            if let Some(msg) = v.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()) {
                Some(msg.to_string())
            } else if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
                Some(msg.to_string())
            } else {
                v.get("error").and_then(|e| e.as_str()).map(|err| err.to_string())
            }
        })
        .unwrap_or_else(|| {
            let s: String = body.chars().take(300).collect();
            if s.trim().is_empty() {
                "unknown error".to_string()
            } else {
                s
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::serve_http;

    #[tokio::test]
    async fn send_json_if_ok_returns_json_only_for_success_responses() {
        let cases = [
            ("200 OK", r#"{"ok":true}"#, true),
            ("500 Internal Server Error", r#"{"ok":true}"#, false),
            ("200 OK", "not json", false),
        ];
        for (status, body, expect_json) in cases {
            let url = serve_http(vec![(status, body.to_string())]).await;
            let value = send_json_if_ok(CLIENT.get(&url)).await;
            assert_eq!(
                value.map(|v| v["ok"].clone()),
                expect_json.then_some(serde_json::Value::Bool(true)),
                "{status} {body}"
            );
        }
    }
}
