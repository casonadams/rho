use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn test_fetch_latest_release_success() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await;
        let body = r#"{"tag_name":"v1.0.0","assets":[{"name":"plugin-mac.tar.gz","browser_download_url":"http://example.com/asset"}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    });

    let client = GitHubClient::with_base_url(format!("http://{addr}"));
    let release = client.fetch_release("casonadams/my-plugin", None).await.unwrap();

    assert_eq!(release.tag_name, "v1.0.0");
    assert_eq!(release.assets.len(), 1);
    assert_eq!(release.assets[0].name, "plugin-mac.tar.gz");
}

#[tokio::test]
async fn test_fetch_tagged_release_success() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        let request = String::from_utf8_lossy(&buf[..n]);
        assert!(request.contains("/releases/tags/v2.1.0"));
        let body = r#"{"tag_name":"v2.1.0","assets":[]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    });

    let client = GitHubClient::with_base_url(format!("http://{addr}"));
    let release = client
        .fetch_release("casonadams/my-plugin", Some("v2.1.0"))
        .await
        .unwrap();

    assert_eq!(release.tag_name, "v2.1.0");
}

#[tokio::test]
async fn test_fetch_release_not_found() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await;
        let body = r#"{"message":"Not Found"}"#;
        let response = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    });

    let client = GitHubClient::with_base_url(format!("http://{addr}"));
    let result = client.fetch_release("casonadams/missing", None).await;

    match result {
        Err(GitHubError::NotFound { repo_slug, .. }) => {
            assert_eq!(repo_slug, "casonadams/missing");
        }
        other => panic!("expected NotFound error, got {:?}", other),
    }
}

#[tokio::test]
async fn test_fetch_release_rate_limited() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await;
        let body = r#"{"message":"API rate limit exceeded"}"#;
        let response = format!(
            "HTTP/1.1 403 Forbidden\r\nx-ratelimit-remaining: 0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    });

    let client = GitHubClient::with_base_url(format!("http://{addr}"));
    let result = client.fetch_release("casonadams/limited", None).await;

    assert!(matches!(result, Err(GitHubError::RateLimited)));
}

#[tokio::test]
async fn test_download_asset_success() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await;
        let payload = b"hello binary data";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.write_all(payload).await.unwrap();
    });

    let client = GitHubClient::new();
    let data = client
        .download_asset(&format!("http://{addr}/asset.tar.gz"))
        .await
        .unwrap();
    assert_eq!(data, b"hello binary data");
}
