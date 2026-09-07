mod body;
mod dns;
#[cfg(test)]
mod tests;

use body::{ReadLimitedParams, read_limited};
use dns::assert_public_dns;
use reqwest::Client;
use rho_harness_core::error::{AppError, Result};
pub use rho_harness_core::net::{
    BRAVE_CHROME_UA, DEFAULT_USER_AGENT, HttpRequest, LYNX_UA, is_private_host, is_private_ip, validate_url,
};
use std::sync::LazyLock;
use std::time::Duration;
use url::Url;

#[derive(Clone, Debug)]
pub struct HttpResponse<T> {
    pub body: T,
    pub content_type: String,
    pub final_url: String,
}

static PUBLIC_CLIENT: LazyLock<Client> = LazyLock::new(|| {
    crate::install_crypto_provider();
    Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
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
    crate::install_crypto_provider();
    Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(10))
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
        rho_harness_core::net::validate_url(raw_url, self.allow_private_network)
    }

    async fn send_request(&self, request: &HttpRequest<'_>, accept_headers: bool) -> Result<reqwest::Response> {
        let valid_url = self.validate_url(request.url)?;
        if !self.allow_private_network {
            assert_public_dns(&valid_url).await?;
        }
        let ua = request.user_agent.unwrap_or(DEFAULT_USER_AGENT);
        let mut req = self
            .client
            .get(valid_url.as_str())
            .header("User-Agent", ua)
            .timeout(Duration::from_secs(request.timeout_sec));
        if accept_headers {
            req = req
                .header(
                    "Accept",
                    "text/html,application/xhtml+xml,application/pdf,application/json,text/plain;q=0.9,*/*;q=0.1",
                )
                .header("Accept-Language", "en-US,en;q=0.8");
        }
        let resp = req
            .send()
            .await
            .map_err(|e| AppError::Tool(format!("HTTP request failed for {}: {e}", request.url)))?;
        let status = resp.status();
        if !status.is_success() {
            let suffix = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .map(|v| format!(" (retry-after: {v})"))
                .unwrap_or_default();
            return Err(AppError::Tool(format!(
                "HTTP error {status}{suffix} from {}",
                request.url
            )));
        }
        Ok(resp)
    }

    pub async fn get_text(&self, request: HttpRequest<'_>) -> Result<HttpResponse<String>> {
        let resp = self.send_request(&request, true).await?;
        let final_url = resp.url().as_str().to_string();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html")
            .to_string();
        let bytes = read_limited(ReadLimitedParams {
            response: resp,
            content_type: &content_type,
            max_bytes: request.max_bytes,
            pdf_max_bytes: request.pdf_max_bytes,
        })
        .await?;
        let body = String::from_utf8_lossy(&bytes).to_string();
        Ok(HttpResponse {
            body,
            content_type,
            final_url,
        })
    }

    pub async fn get_bytes(&self, request: HttpRequest<'_>) -> Result<HttpResponse<Vec<u8>>> {
        let resp = self.send_request(&request, false).await?;
        let final_url = resp.url().as_str().to_string();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = read_limited(ReadLimitedParams {
            response: resp,
            content_type: &content_type,
            max_bytes: request.max_bytes,
            pdf_max_bytes: request.pdf_max_bytes,
        })
        .await?;
        Ok(HttpResponse {
            body: bytes,
            content_type,
            final_url,
        })
    }
}
