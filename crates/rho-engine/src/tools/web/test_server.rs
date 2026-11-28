//! Loopback HTTP server for web tool tests: answers the first connection with a canned response.

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

pub(crate) struct CannedResponse {
    pub status: &'static str,
    pub content_type: &'static str,
    pub body: String,
    pub delay: Duration,
}

impl CannedResponse {
    pub fn new(status: &'static str, content_type: &'static str, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type,
            body: body.into(),
            delay: Duration::ZERO,
        }
    }
}

/// Serves `response` on a loopback port. Returns the base URL, ending in `/`, and a channel that
/// receives the request head and body the server read.
pub async fn serve_once(response: CannedResponse) -> (String, oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let _ = tx.send(read_request(&mut socket).await);
        tokio::time::sleep(response.delay).await;
        let head = format!(
            "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.status,
            response.content_type,
            response.body.len()
        );
        let _ = socket.write_all(format!("{head}{}", response.body).as_bytes()).await;
        let _ = socket.shutdown().await;
    });
    (format!("http://{addr}/"), rx)
}

async fn read_request(socket: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = socket.read(&mut chunk).await.unwrap();
        if n == 0 {
            return String::from_utf8_lossy(&buf).to_string();
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, val) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| val.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while buf.len() < header_end + content_length {
        let n = socket.read(&mut chunk).await.unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    String::from_utf8_lossy(&buf).to_string()
}
