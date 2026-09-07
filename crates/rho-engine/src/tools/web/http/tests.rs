use super::*;

#[test]
fn test_private_host_detection() {
    let cases = [
        ("127.0.0.1", true),
        ("localhost", true),
        ("192.168.1.1", true),
        ("10.0.0.5", true),
        ("example.com", false),
        ("8.8.8.8", false),
    ];
    for (host, expected) in cases {
        assert_eq!(is_private_host(host), expected);
    }
}

#[test]
fn blocks_private_urls_before_network_io() {
    let client = HttpClient::new(false).unwrap();
    for url in ["http://127.0.0.1/", "http://[::1]/", "http://host.local/"] {
        assert!(client.validate_url(url).is_err(), "{url}");
    }
}

#[test]
fn blocks_credential_urls() {
    let client = HttpClient::new(true).unwrap();
    assert!(client.validate_url("http://user:pass@example.com/").is_err());
    assert!(client.validate_url("https://admin@example.com/").is_err());
}

#[tokio::test]
async fn response_body_rejects_oversized_content() {
    let url = spawn_response_server("abcdefgh", Duration::ZERO).await;
    let client = HttpClient::new(true).unwrap();
    let err = client
        .get_text(HttpRequest {
            url: &url,
            user_agent: None,
            timeout_sec: 2,
            max_bytes: 4,
            pdf_max_bytes: None,
        })
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("too large") || err.to_string().contains("exceeded"),
        "{err}"
    );
}

#[tokio::test]
async fn response_body_reads_under_size_limit() {
    let url = spawn_response_server("hello", Duration::ZERO).await;
    let client = HttpClient::new(true).unwrap();
    let resp = client
        .get_text(HttpRequest {
            url: &url,
            user_agent: None,
            timeout_sec: 2,
            max_bytes: 100,
            pdf_max_bytes: None,
        })
        .await
        .unwrap();
    assert_eq!(resp.body, "hello");
    assert_eq!(resp.content_type, "text/plain");
    assert!(resp.final_url.starts_with("http://"));
}

#[tokio::test]
async fn request_respects_per_call_timeout() {
    let url = spawn_response_server("late", Duration::from_millis(100)).await;
    let client = HttpClient::new(true).unwrap();
    let error = client
        .get_text(HttpRequest {
            url: &url,
            user_agent: None,
            timeout_sec: 0,
            max_bytes: 100,
            pdf_max_bytes: None,
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("HTTP request failed"));
}

async fn spawn_response_server(body: &'static str, delay: Duration) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).await;
        tokio::time::sleep(delay).await;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes()).await;
    });
    format!("http://{address}/")
}
