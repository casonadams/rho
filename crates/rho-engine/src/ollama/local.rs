//! Local Ollama server: host resolution and context-window queries.

use crate::http::{CLIENT, get_json};
use rho_harness_core::error::Result;
use serde_json::Value;
use std::time::Duration;

const DEFAULT_HOST: &str = "http://127.0.0.1:11434";
const DEFAULT_PORT: u16 = 11434;
const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

pub fn local_host() -> String {
    normalize_host(std::env::var("OLLAMA_HOST").ok().as_deref())
}

/// Mirrors Ollama's `envconfig.ConnectableHost`: a bare `host[:port]` means plain HTTP on port
/// 11434, and an unspecified bind address (`0.0.0.0`, `::`) connects through loopback.
fn normalize_host(raw: Option<&str>) -> String {
    let raw = raw.map(str::trim).unwrap_or_default().trim_end_matches('/');
    if raw.is_empty() {
        return DEFAULT_HOST.to_string();
    }
    let has_scheme = raw.contains("://");
    let candidate = if has_scheme {
        raw.to_string()
    } else {
        format!("http://{raw}")
    };
    let Ok(mut url) = url::Url::parse(&candidate) else {
        return candidate;
    };
    if !has_scheme && url.port().is_none() {
        let _ = url.set_port(Some(DEFAULT_PORT));
    }
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) if ip.is_unspecified() => Some("127.0.0.1"),
        Some(url::Host::Ipv6(ip)) if ip.is_unspecified() => Some("[::1]"),
        _ => None,
    };
    if let Some(host) = loopback {
        let _ = url.set_host(Some(host));
    }
    url.as_str().trim_end_matches('/').to_string()
}

/// Context window a model is configured for: `num_ctx` from its Modelfile, else its trained maximum.
pub(crate) async fn show_context_length(host: &str, model: &str) -> Option<usize> {
    let request = CLIENT
        .post(format!("{host}/api/show"))
        .timeout(QUERY_TIMEOUT)
        .json(&serde_json::json!({ "model": model }));
    parse_show_context(&get_json(request, "Ollama /api/show").await.ok()?)
}

/// Context window the server allocated for `model`; `Ok(None)` while the model is not loaded.
pub(crate) async fn loaded_context_length(host: &str, model: &str) -> Result<Option<usize>> {
    let request = CLIENT.get(format!("{host}/api/ps")).timeout(QUERY_TIMEOUT);
    Ok(parse_ps_context(&get_json(request, "Ollama /api/ps").await?, model))
}

fn parse_show_context(body: &Value) -> Option<usize> {
    let num_ctx = body.get("parameters").and_then(Value::as_str).and_then(parse_num_ctx);
    num_ctx.or_else(|| trained_context_length(body.get("model_info")?.as_object()?))
}

fn parse_num_ctx(parameters: &str) -> Option<usize> {
    parameters.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some("num_ctx"), Some(value)) => value.parse().ok(),
            _ => None,
        }
    })
}

fn trained_context_length(model_info: &serde_json::Map<String, Value>) -> Option<usize> {
    let arch_key = model_info
        .get("general.architecture")
        .and_then(Value::as_str)
        .map(|arch| format!("{arch}.context_length"));
    let value = match arch_key.and_then(|key| model_info.get(&key)) {
        Some(value) => value,
        None => model_info.iter().find(|(key, _)| key.ends_with(".context_length"))?.1,
    };
    value.as_u64().and_then(|n| usize::try_from(n).ok())
}

fn parse_ps_context(body: &Value, model: &str) -> Option<usize> {
    let wanted = with_default_tag(model);
    body.get("models")?
        .as_array()?
        .iter()
        .find(|entry| {
            entry
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| with_default_tag(name) == wanted)
        })?
        .get("context_length")?
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
}

fn with_default_tag(model: &str) -> String {
    if model.contains(':') {
        model.to_string()
    } else {
        format!("{model}:latest")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::serve_http;
    use serde_json::json;

    #[test]
    fn normalize_host_follows_ollama_host_rules() {
        let cases = [
            (None, "http://127.0.0.1:11434"),
            (Some("  "), "http://127.0.0.1:11434"),
            (Some("http://localhost:11434/"), "http://localhost:11434"),
            (Some("127.0.0.1"), "http://127.0.0.1:11434"),
            (Some("box.lan:9000"), "http://box.lan:9000"),
            (Some("0.0.0.0"), "http://127.0.0.1:11434"),
            (Some("[::]:8080"), "http://[::1]:8080"),
            (Some("https://ollama.example"), "https://ollama.example"),
            (Some("http://gpu.lan/ollama"), "http://gpu.lan/ollama"),
        ];
        for (raw, expected) in cases {
            assert_eq!(normalize_host(raw), expected, "{raw:?}");
        }
    }

    #[test]
    fn show_context_prefers_num_ctx_then_architecture_key() {
        let cases = [
            (
                json!({ "parameters": "stop \"<|im_start|>\"\nnum_ctx 16384", "model_info": { "llama.context_length": 8192 } }),
                Some(16_384),
            ),
            (
                json!({ "model_info": {
                    "general.architecture": "qwen2",
                    "clip.context_length": 77,
                    "qwen2.context_length": 32_768
                } }),
                Some(32_768),
            ),
            (
                json!({ "model_info": { "qwen3_5.context_length": 262_144 } }),
                Some(262_144),
            ),
            (json!({ "parameters": "temperature 0.7", "model_info": {} }), None),
            (json!({}), None),
        ];
        for (body, expected) in cases {
            assert_eq!(parse_show_context(&body), expected, "{body}");
        }
    }

    #[test]
    fn ps_context_matches_model_with_implicit_latest_tag() {
        let body = json!({ "models": [
            { "name": "qwen2.5-coder:7b", "context_length": 4096 },
            { "name": "inkling-2-mini:latest", "context_length": 262_144 }
        ] });
        assert_eq!(parse_ps_context(&body, "inkling-2-mini"), Some(262_144));
        assert_eq!(parse_ps_context(&body, "inkling-2-mini:latest"), Some(262_144));
        assert_eq!(parse_ps_context(&body, "qwen2.5-coder:7b"), Some(4096));
        assert_eq!(parse_ps_context(&body, "qwen2.5-coder"), None);
        assert_eq!(parse_ps_context(&json!({ "models": [] }), "inkling-2-mini"), None);
    }

    #[tokio::test]
    async fn context_queries_read_server_and_reject_failures() {
        let show = json!({ "model_info": { "llama.context_length": 8192 } }).to_string();
        let host = serve_http(vec![("200 OK", show)]).await;
        assert_eq!(show_context_length(&host, "llama3.2").await, Some(8192));

        let ps = json!({ "models": [{ "name": "llama3.2:latest", "context_length": 4096 }] }).to_string();
        let host = serve_http(vec![("200 OK", ps), ("200 OK", r#"{"models":[]}"#.to_string())]).await;
        assert_eq!(loaded_context_length(&host, "llama3.2").await.unwrap(), Some(4096));
        assert_eq!(loaded_context_length(&host, "llama3.2").await.unwrap(), None);

        let host = serve_http(vec![
            ("404 Not Found", "{}".to_string()),
            ("500 Internal Server Error", "{}".to_string()),
            ("200 OK", "not json".to_string()),
        ])
        .await;
        assert_eq!(show_context_length(&host, "missing").await, None);
        let server_error = loaded_context_length(&host, "missing").await.unwrap_err();
        assert!(
            server_error.to_string().contains("/api/ps returned HTTP 500"),
            "{server_error}"
        );
        let bad_body = loaded_context_length(&host, "missing").await.unwrap_err();
        assert!(bad_body.to_string().contains("invalid JSON"), "{bad_body}");
    }
}
