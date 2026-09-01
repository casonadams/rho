use super::*;

#[test]
fn test_private_host_detection() {
    assert!(is_private_host("127.0.0.1"));
    assert!(is_private_host("localhost"));
    assert!(is_private_host("192.168.1.1"));
    assert!(is_private_host("10.0.0.5"));
    assert!(!is_private_host("example.com"));
    assert!(!is_private_host("8.8.8.8"));
}

#[test]
fn blocks_private_urls_before_network_io() {
    let client = HttpClient::new(false).unwrap();
    for url in ["http://127.0.0.1/", "http://[::1]/", "http://host.local/"] {
        assert!(client.validate_url(url).is_err(), "{url}");
    }
}

#[tokio::test]
async fn response_body_respects_size_limit() {
    let url = spawn_response_server("abcdefgh", Duration::ZERO).await;
    let client = HttpClient::new(true).unwrap();
    let (body, _) = client
        .get_text(HttpRequest {
            url: &url,
            user_agent: None,
            timeout_sec: 2,
            max_bytes: 4,
        })
        .await
        .unwrap();
    assert_eq!(body, "abcd");
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
