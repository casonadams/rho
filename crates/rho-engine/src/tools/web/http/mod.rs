use futures::StreamExt;
use reqwest::Client;
use rho_core::error::{AppError, Result};
pub use rho_core::net::{BRAVE_CHROME_UA, DEFAULT_USER_AGENT, HttpRequest, LYNX_UA, is_private_host, validate_url};
use std::sync::LazyLock;
use std::time::Duration;
use url::Url;

#[cfg(test)]
mod tests;

async fn read_limited(response: reqwest::Response, max_bytes: usize) -> std::result::Result<Vec<u8>, reqwest::Error> {
    let capacity = max_bytes.min(256 * 1024);
    let mut body: Vec<u8> = Vec::with_capacity(capacity);
    let mut stream = response.bytes_stream();
    while body.len() < max_bytes {
        let Some(chunk) = stream.next().await else {
            break;
        };
        let chunk = chunk?;
        let remaining = max_bytes - body.len();
        let take = remaining.min(chunk.len());
        body.extend_from_slice(&chunk[..take]);
    }
    Ok(body)
}

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

        let bytes = read_limited(resp, request.max_bytes)
            .await
            .map_err(|e| AppError::Tool(format!("Failed to read response body from {}: {e}", request.url)))?;

        let body = String::from_utf8_lossy(&bytes).to_string();
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

        let bytes = read_limited(resp, request.max_bytes)
            .await
            .map_err(|e| AppError::Tool(format!("Failed to read body from {}: {e}", request.url)))?;

        Ok((bytes, content_type))
    }
}
