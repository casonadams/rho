use reqwest::Client;
use rho_core::error::{AppError, Result};
pub use rho_core::net::{BRAVE_CHROME_UA, DEFAULT_USER_AGENT, HttpRequest, LYNX_UA, is_private_host, validate_url};
use std::sync::LazyLock;
use std::time::Duration;
use url::Url;

static PUBLIC_CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.error("too many redirects");
            }
            if let Some(host) = attempt.url().host_str()
                && is_private_host(host)
            {
                return attempt.error("redirect to private network host blocked");
            }
            attempt.follow()
        }))
        .build()
        .expect("Failed to build public HTTP client")
});

static PRIVATE_CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .expect("Failed to build private HTTP client")
});

#[derive(Clone)]
pub struct HttpClient {
    pub client: Client,
    pub allow_private_network: bool,
}

impl HttpClient {
    pub fn new(allow_private_network: bool) -> Result<Self> {
        let client = if allow_private_network {
            PRIVATE_CLIENT.clone()
        } else {
            PUBLIC_CLIENT.clone()
        };
        Ok(Self {
            client,
            allow_private_network,
        })
    }

    pub fn validate_url(&self, raw_url: &str) -> Result<Url> {
        rho_core::net::validate_url(raw_url, self.allow_private_network)
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
