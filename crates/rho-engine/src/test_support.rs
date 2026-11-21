use std::path::PathBuf;

pub(crate) fn temp_dir(label: &str) -> PathBuf {
    tempfile::Builder::new()
        .prefix(&format!("rho-{label}-"))
        .tempdir()
        .unwrap()
        .keep()
}

pub(crate) fn temp_auth_store() -> (tempfile::TempDir, crate::auth::AuthStore) {
    let dir = tempfile::tempdir().unwrap();
    let auth_store = crate::auth::AuthStore::load(dir.path().join("auth.json")).unwrap();
    (dir, auth_store)
}

/// A non-expiring OAuth login with no refresh token.
pub(crate) fn oauth_login(
    token: &str,
    account_id: Option<&str>,
    email: Option<&str>,
) -> rho_harness_core::auth::StoredCredential {
    rho_harness_core::auth::StoredCredential::OAuth {
        access_token: token.to_string(),
        refresh_token: None,
        expires_at_ms: None,
        account_id: account_id.map(str::to_string),
        account_email: email.map(str::to_string),
    }
}

pub(crate) fn write_file(path: &std::path::Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

pub(crate) fn assert_quota_cases(
    cases: &[(&str, &str, Option<&str>, Option<&str>)],
    now: chrono::DateTime<chrono::Utc>,
    parse: impl Fn(&serde_json::Value, Option<&str>, chrono::DateTime<chrono::Utc>) -> Option<String>,
) {
    for (name, json, model, expected) in cases {
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(parse(&value, *model, now), expected.map(str::to_string), "{name}");
    }
}

/// Serves one HTTP response per accepted connection, in order, on a loopback port.
/// Returns the base URL (`http://127.0.0.1:<port>`). Each request body is drained by its
/// `Content-Length` before replying, so clients that stream a form body never see a reset.
pub(crate) async fn serve_http(responses: Vec<(&'static str, String)>) -> String {
    serve_http_recording(responses).await.0
}

/// Like [`serve_http`], also recording each request's line and headers (lowercased).
pub(crate) async fn serve_http_recording(
    responses: Vec<(&'static str, String)>,
) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = requests.clone();
    tokio::spawn(async move {
        for (status, body) in responses {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut raw = Vec::new();
            let mut chunk = [0u8; 1024];
            let header_end = loop {
                if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos + 4;
                }
                match socket.read(&mut chunk).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => raw.extend_from_slice(&chunk[..n]),
                }
            };
            let headers = String::from_utf8_lossy(&raw[..header_end]).to_ascii_lowercase();
            recorded.lock().unwrap().push(headers.clone());
            let content_length = headers
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            while raw.len() < header_end + content_length {
                match socket.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => raw.extend_from_slice(&chunk[..n]),
                }
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    (format!("http://{addr}"), requests)
}

/// Accepts one connection per raw response string, reads the request once, and writes the
/// response verbatim. Returns the base URL (`http://127.0.0.1:<port>`).
pub(crate) async fn serve_raw_http(responses: Vec<String>) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for response in responses {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.flush().await;
            }
        }
    });
    format!("http://{addr}")
}

/// Token provider with scripted `token()` and `force_refresh()` results.
pub(crate) struct ScriptedTokenProvider {
    token: Result<String, String>,
    refresh: Result<String, String>,
    pub refresh_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl ScriptedTokenProvider {
    pub(crate) fn new(token: Result<&str, &str>, refresh: Result<&str, &str>) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            token: token.map(str::to_string).map_err(str::to_string),
            refresh: refresh.map(str::to_string).map_err(str::to_string),
            refresh_count: std::sync::Arc::default(),
        })
    }
}

#[async_trait::async_trait]
impl crate::auth::token::TokenProvider for ScriptedTokenProvider {
    async fn token(&self) -> Result<String, String> {
        self.token.clone()
    }
    async fn force_refresh(&self) -> Result<String, String> {
        self.refresh_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.refresh.clone()
    }
}
