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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
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
    format!("http://{addr}")
}
