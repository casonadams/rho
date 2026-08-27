use crate::error::{AppError, Result};
use reqwest::Client;
use std::time::Duration;
use url::Url;

pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0 Safari/537.36 rho/0.1.0";
pub const BRAVE_CHROME_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
pub const LYNX_UA: &str = "Lynx/2.9.3 libwww-FM/2.14 SSL-MM/1.4.1 OpenSSL/4.0.0";

#[derive(Clone)]
pub struct HttpRequest<'a> {
    pub url: &'a str,
    pub user_agent: Option<&'a str>,
    pub timeout_sec: u64,
    pub max_bytes: usize,
}

#[derive(Clone)]
pub struct HttpClient {
    pub client: Client,
    pub allow_private_network: bool,
}

impl HttpClient {
    pub fn new(allow_private_network: bool) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| AppError::Tool(format!("Failed to build HTTP client: {e}")))?;
        Ok(Self {
            client,
            allow_private_network,
        })
    }

    pub fn validate_url(&self, raw_url: &str) -> Result<Url> {
        let parsed = Url::parse(raw_url).map_err(|e| AppError::Tool(format!("Invalid URL '{raw_url}': {e}")))?;

        match parsed.scheme() {
            "http" | "https" => {}
            other => return Err(AppError::Tool(format!("Unsupported URL scheme: '{other}'"))),
        }

        if !self.allow_private_network
            && let Some(host) = parsed.host_str()
            && is_private_host(host)
        {
            return Err(AppError::Tool(format!(
                "Access to private/local network host '{host}' is blocked for security"
            )));
        }

        Ok(parsed)
    }

    pub async fn get_text(&self, request: HttpRequest<'_>) -> Result<(String, String)> {
        let valid_url = self.validate_url(request.url)?;
        let ua = request.user_agent.unwrap_or(DEFAULT_USER_AGENT);

        let resp = self
            .client
            .get(valid_url.as_str())
            .header("User-Agent", ua)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/pdf,application/json,text/plain;q=0.9,*/*;q=0.1",
            )
            .header("Accept-Language", "en-US,en;q=0.8")
            .timeout(Duration::from_secs(request.timeout_sec))
            .send()
            .await
            .map_err(|e| AppError::Tool(format!("HTTP request failed for {}: {e}", request.url)))?;

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html")
            .to_string();

        let status = resp.status();
        if !status.is_success() {
            return Err(AppError::Tool(format!("HTTP error {status} from {}", request.url)));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| AppError::Tool(format!("Failed to read response body from {}: {e}", request.url)))?;

        let slice = if bytes.len() > request.max_bytes {
            &bytes[..request.max_bytes]
        } else {
            &bytes[..]
        };

        let body = String::from_utf8_lossy(slice).to_string();
        Ok((body, content_type))
    }

    pub async fn get_bytes(&self, request: HttpRequest<'_>) -> Result<(Vec<u8>, String)> {
        let valid_url = self.validate_url(request.url)?;
        let ua = request.user_agent.unwrap_or(DEFAULT_USER_AGENT);

        let resp = self
            .client
            .get(valid_url.as_str())
            .header("User-Agent", ua)
            .timeout(Duration::from_secs(request.timeout_sec))
            .send()
            .await
            .map_err(|e| AppError::Tool(format!("HTTP request failed for {}: {e}", request.url)))?;

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();

        let status = resp.status();
        if !status.is_success() {
            return Err(AppError::Tool(format!("HTTP error {status} from {}", request.url)));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| AppError::Tool(format!("Failed to read body from {}: {e}", request.url)))?;

        let truncated = if bytes.len() > request.max_bytes {
            bytes[..request.max_bytes].to_vec()
        } else {
            bytes.to_vec()
        };

        Ok((truncated, content_type))
    }
}

pub fn is_private_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let lower = host.to_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".local") {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.octets()[0] == 0
            }
            std::net::IpAddr::V6(v6) => v6.is_loopback(),
        };
    }
    false
}

#[cfg(test)]
mod tests {
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
}
