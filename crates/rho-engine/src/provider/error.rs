//! Maps provider HTTP failures and stream error events onto typed `AppError`s, so the engine decides
//! retries, compaction, and content-filter handling by variant instead of matching message text.

use rho_harness_core::error::AppError;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpFailure {
    /// The request never got an HTTP response (DNS, connect, TLS, or a dropped connection).
    Transport(String),
    Status {
        status: u16,
        body: String,
        retry_after: Option<Duration>,
    },
    /// The request could not be built or authorized locally.
    Local(String),
}

pub type HttpResult = Result<reqwest::Response, HttpFailure>;

impl HttpFailure {
    pub fn status(status: u16, body: impl Into<String>) -> Self {
        Self::Status {
            status,
            body: body.into(),
            retry_after: None,
        }
    }

    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::Status { status, .. } => Some(*status),
            Self::Transport(_) | Self::Local(_) => None,
        }
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Status { retry_after, .. } => *retry_after,
            Self::Transport(_) | Self::Local(_) => None,
        }
    }

    pub fn body(&self) -> &str {
        match self {
            Self::Status { body, .. } => body,
            Self::Transport(message) | Self::Local(message) => message,
        }
    }

    pub fn into_app_error(self, friendly: fn(Option<u16>, &str) -> String) -> AppError {
        let message = friendly(self.status_code(), self.body());
        match self {
            Self::Transport(_) => AppError::Network(message),
            Self::Local(_) => AppError::Provider(message),
            Self::Status {
                status,
                body,
                retry_after,
            } => classify_status(status, &body, retry_after, message),
        }
    }
}

/// Longest provider-requested wait rho will sit through; longer waits (quota resets) fail instead.
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

pub(crate) fn is_hard_quota(body: &str) -> bool {
    const MARKERS: &[&str] = &["Individual quota reached", "insufficient_quota", "usage_limit_reached"];
    MARKERS.iter().any(|marker| body.contains(marker))
}

fn classify_status(status: u16, body: &str, retry_after: Option<Duration>, message: String) -> AppError {
    let wait_too_long = retry_after.is_some_and(|wait| wait > MAX_RETRY_AFTER);
    match status {
        429 if is_hard_quota(body) || wait_too_long => AppError::Provider(message),
        429 => AppError::RateLimited { message, retry_after },
        408 | 500 | 502 | 503 | 504 | 529 if wait_too_long => AppError::Provider(message),
        408 | 500 | 502 | 503 | 504 | 529 => AppError::ProviderUnavailable { message, retry_after },
        _ => classify_message(message, body),
    }
}

/// Classifies an error reported inside an otherwise successful stream, from its error type when the
/// provider sends one, else from the message.
pub fn classify_stream_error(error_type: Option<&str>, message: String) -> AppError {
    match error_type {
        Some("rate_limit_error" | "rate_limit_exceeded") => AppError::RateLimited {
            message,
            retry_after: None,
        },
        Some("overloaded_error" | "api_error" | "timeout_error" | "server_error" | "server_is_overloaded") => {
            AppError::ProviderUnavailable {
                message,
                retry_after: None,
            }
        }
        Some("insufficient_quota" | "usage_limit_reached") => AppError::Provider(message),
        _ => classify_message(message, ""),
    }
}

/// Classifies from the user-facing message and the raw provider body, since friendly messages drop error codes.
fn classify_message(message: String, body: &str) -> AppError {
    let lower = format!("{message}\n{body}").to_lowercase();
    if is_context_overflow_text(&lower) {
        AppError::ContextOverflow(message)
    } else if lower.contains("content_filter") || lower.contains("content filter") {
        AppError::ContentFiltered
    } else {
        AppError::Provider(message)
    }
}

fn is_context_overflow_text(lower: &str) -> bool {
    const SIGNALS: &[&str] = &[
        "context_length_exceeded",
        "context length exceeded",
        "context window exceeded",
        "context window is full",
        "context_window_exceeded",
        "maximum context length",
        "exceeds the context window",
        "exceeds maximum context",
        "prompt is too long",
        "prompt exceeds",
        "prompt_length_exceeded",
        "input token count exceeds",
        "total input tokens exceed",
        "token limit exceeded",
        "too many tokens",
        "request payload size exceeds the limit",
    ];
    SIGNALS.iter().any(|needle| lower.contains(needle))
}

/// Reads `retry-after-ms`, then `retry-after` as delay-seconds or an HTTP-date (RFC 9110 10.2.3).
pub fn parse_retry_after(headers: &reqwest::header::HeaderMap, now: chrono::DateTime<chrono::Utc>) -> Option<Duration> {
    let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim);
    let ms = text("retry-after-ms")
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|ms| ms.is_finite() && *ms >= 0.0);
    if let Some(ms) = ms {
        return Some(Duration::try_from_secs_f64(ms / 1000.0).unwrap_or(Duration::MAX));
    }
    let value = text("retry-after")?;
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at = parse_http_date(value)?;
    Some((at - now).to_std().unwrap_or(Duration::ZERO))
}

/// IMF-fixdate plus the obsolete RFC 850 and asctime forms recipients must accept (RFC 9110 5.6.7).
fn parse_http_date(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Ok(at) = chrono::DateTime::parse_from_rfc2822(value) {
        return Some(at.to_utc());
    }
    ["%A, %d-%b-%y %H:%M:%S GMT", "%a %b %e %H:%M:%S %Y"]
        .iter()
        .find_map(|format| chrono::NaiveDateTime::parse_from_str(value, format).ok())
        .map(|at| at.and_utc())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    fn plain(_: Option<u16>, body: &str) -> String {
        body.to_string()
    }

    fn failure(status: u16, body: &str, retry_after: Option<u64>) -> AppError {
        HttpFailure::Status {
            status,
            body: body.to_string(),
            retry_after: retry_after.map(Duration::from_secs),
        }
        .into_app_error(plain)
    }

    #[test]
    fn http_statuses_map_to_typed_errors() {
        assert!(matches!(
            failure(429, "slow down", Some(7)),
            AppError::RateLimited { retry_after: Some(d), .. } if d == Duration::from_secs(7)
        ));
        for status in [408, 500, 502, 503, 504, 529] {
            assert!(
                matches!(failure(status, "busy", None), AppError::ProviderUnavailable { .. }),
                "{status}"
            );
        }
        assert!(matches!(
            failure(400, "prompt is too long: 201234 tokens > 200000 maximum", None),
            AppError::ContextOverflow(_)
        ));
        assert!(matches!(
            failure(400, r#"{"error":{"code":"content_filter"}}"#, None),
            AppError::ContentFiltered
        ));
        assert!(matches!(failure(400, "bad request", None), AppError::Provider(_)));
        assert!(matches!(
            HttpFailure::Transport("connection reset".into()).into_app_error(plain),
            AppError::Network(_)
        ));
        assert!(matches!(
            HttpFailure::Local("no token".into()).into_app_error(plain),
            AppError::Provider(_)
        ));
    }

    #[tokio::test]
    async fn rate_limit_responses_carry_retry_after_from_the_wire() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/messages", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let body = r#"{"error":{"type":"rate_limit_error","message":"slow down"}}"#;
            let reply = format!(
                "HTTP/1.1 429 Too Many Requests\r\nretry-after: 3\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        });

        let failure = crate::http::post_json_checked(&url, HeaderMap::new(), &serde_json::json!({}))
            .await
            .unwrap_err();
        let error = failure.into_app_error(plain);

        assert!(error.is_transient());
        assert_eq!(error.retry_after(), Some(Duration::from_secs(3)));
        assert!(error.to_string().contains("slow down"), "{error}");
    }

    #[test]
    fn overflow_messages_from_each_provider_are_recognized() {
        let overflow = [
            "InvalidRequestError: This model's maximum context length is 128000 tokens.",
            "error: prompt is too long: 205000 tokens > 200000 maximum tokens",
            "ResourceExhausted: input token count exceeds limit",
            "context_length_exceeded",
            "context window exceeded",
            "Request payload size exceeds the limit: 1048576 bytes",
            "exceeds the context window of 128000 tokens",
        ];
        for msg in overflow {
            assert!(matches!(failure(400, msg, None), AppError::ContextOverflow(_)), "{msg}");
        }
        for msg in [
            "Connection reset by peer",
            "Unauthorized 401",
            "RESOURCE_EXHAUSTED: Quota exceeded for quota metric 'generate_content_requests'",
        ] {
            assert!(matches!(failure(400, msg, None), AppError::Provider(_)), "{msg}");
        }
    }

    #[test]
    fn quota_resets_and_long_waits_are_not_retried() {
        for body in [
            "Individual quota reached",
            r#"{"error":{"code":"insufficient_quota"}}"#,
            r#"{"error":{"type":"usage_limit_reached"}}"#,
        ] {
            assert!(matches!(failure(429, body, None), AppError::Provider(_)), "{body}");
        }
        assert!(matches!(failure(429, "over limit", Some(3600)), AppError::Provider(_)));
        assert!(matches!(failure(503, "down", Some(3600)), AppError::Provider(_)));
        assert!(matches!(
            classify_stream_error(Some("insufficient_quota"), "out of credit".into()),
            AppError::Provider(_)
        ));
        assert!(!failure(400, "RESOURCE_EXHAUSTED: Quota exceeded for quota metric", None).is_transient());
    }

    #[test]
    fn error_codes_in_the_raw_body_classify_even_when_the_message_drops_them() {
        let friendly = |_: Option<u16>, _: &str| "Request rejected.".to_string();
        let filtered = HttpFailure::status(400, r#"{"error":{"code":"content_filter","message":"no"}}"#);
        assert!(matches!(filtered.into_app_error(friendly), AppError::ContentFiltered));
        let overflow = HttpFailure::status(400, r#"{"error":{"code":"context_length_exceeded"}}"#);
        assert!(matches!(
            overflow.into_app_error(friendly),
            AppError::ContextOverflow(_)
        ));
    }

    #[test]
    fn stream_errors_use_the_provider_error_type() {
        let cases = [
            (Some("overloaded_error"), "Overloaded", true),
            (Some("rate_limit_error"), "slow", true),
            (Some("invalid_request_error"), "bad", false),
            (None, "network blip", false),
        ];
        for (kind, message, transient) in cases {
            assert_eq!(
                classify_stream_error(kind, message.into()).is_transient(),
                transient,
                "{kind:?}"
            );
        }
        assert!(matches!(
            classify_stream_error(None, "context_length_exceeded".into()),
            AppError::ContextOverflow(_)
        ));
    }

    #[test]
    fn retry_after_accepts_milliseconds_seconds_and_http_dates() {
        let now = chrono::DateTime::parse_from_rfc3339("2015-10-21T07:28:00Z")
            .unwrap()
            .to_utc();
        let headers = |pairs: &[(&'static str, &'static str)]| {
            let mut map = HeaderMap::new();
            for (name, value) in pairs {
                map.insert(*name, HeaderValue::from_static(value));
            }
            map
        };
        let cases = [
            (headers(&[("retry-after", "12")]), Some(Duration::from_secs(12))),
            (
                headers(&[("retry-after-ms", "1500"), ("retry-after", "9")]),
                Some(Duration::from_millis(1500)),
            ),
            (
                headers(&[("retry-after", "Wed, 21 Oct 2015 07:28:30 GMT")]),
                Some(Duration::from_secs(30)),
            ),
            (
                headers(&[("retry-after", "Wed, 21 Oct 2015 07:27:00 GMT")]),
                Some(Duration::ZERO),
            ),
            (headers(&[("retry-after-ms", "1e30")]), Some(Duration::MAX)),
            (headers(&[("retry-after-ms", "-5")]), None),
            (
                headers(&[("retry-after-ms", "-5"), ("retry-after", "4")]),
                Some(Duration::from_secs(4)),
            ),
            (
                headers(&[("retry-after", "Wednesday, 21-Oct-15 07:28:20 GMT")]),
                Some(Duration::from_secs(20)),
            ),
            (
                headers(&[("retry-after", "Wed Oct 21 07:28:10 2015")]),
                Some(Duration::from_secs(10)),
            ),
            (headers(&[("retry-after", "soon")]), None),
            (HeaderMap::new(), None),
        ];
        for (map, expected) in cases {
            assert_eq!(parse_retry_after(&map, now), expected, "{map:?}");
        }
    }
}
