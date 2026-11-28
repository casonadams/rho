mod body;
mod dns;
#[cfg(test)]
mod tests;

pub(crate) use body::read_limited_until;
use body::{ReadLimitedParams, read_limited};
use dns::PublicDns;
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

#[derive(Clone, Debug)]
pub(crate) struct HttpPostResponse {
    pub status: reqwest::StatusCode,
    pub content_type: String,
    pub body: String,
}

pub(crate) struct PostJsonRequest<'a> {
    pub url: &'a str,
    pub body: &'a serde_json::Value,
    pub headers: &'a [(&'a str, &'a str)],
    pub timeout_sec: u64,
    pub max_bytes: usize,
}

fn sanitize_url_for_error(url: &Url) -> String {
    let mut sanitized = url.clone();
    sanitized.set_query(None);
    sanitized.set_fragment(None);
    let _ = sanitized.set_password(None);
    sanitized.to_string()
}

static PUBLIC_CLIENT: LazyLock<Client> = LazyLock::new(|| {
    crate::install_crypto_provider();
    Client::builder()
        .no_proxy()
        .dns_resolver(PublicDns)
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

    fn request_builder(
        &self,
        method: reqwest::Method,
        url: &Url,
        user_agent: &str,
        timeout_sec: u64,
    ) -> reqwest::RequestBuilder {
        self.client
            .request(method, url.as_str())
            .header("User-Agent", user_agent)
            .timeout(Duration::from_secs(timeout_sec))
    }

    async fn send_request(&self, request: &HttpRequest<'_>, accept_headers: bool) -> Result<reqwest::Response> {
        let valid_url = self.validate_url(request.url)?;
        let ua = request.user_agent.unwrap_or(DEFAULT_USER_AGENT);
        let mut req = self.request_builder(reqwest::Method::GET, &valid_url, ua, request.timeout_sec);
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

    async fn read_response(
        resp: reqwest::Response,
        request: &HttpRequest<'_>,
        default_content_type: &str,
    ) -> Result<HttpResponse<Vec<u8>>> {
        let final_url = resp.url().as_str().to_string();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(default_content_type)
            .to_string();
        let body = read_limited(ReadLimitedParams {
            response: resp,
            content_type: &content_type,
            max_bytes: request.max_bytes,
            pdf_max_bytes: request.pdf_max_bytes,
        })
        .await?;
        Ok(HttpResponse {
            body,
            content_type,
            final_url,
        })
    }

    pub async fn get_text(&self, request: HttpRequest<'_>) -> Result<HttpResponse<String>> {
        let resp = self.send_request(&request, true).await?;
        let bytes = Self::read_response(resp, &request, "text/html").await?;
        Ok(HttpResponse {
            body: String::from_utf8_lossy(&bytes.body).into_owned(),
            content_type: bytes.content_type,
            final_url: bytes.final_url,
        })
    }

    pub(crate) async fn get_bytes(&self, request: HttpRequest<'_>) -> Result<HttpResponse<Vec<u8>>> {
        let resp = self.send_request(&request, false).await?;
        Self::read_response(resp, &request, "application/octet-stream").await
    }

    async fn send_post_json(&self, request: &PostJsonRequest<'_>) -> Result<reqwest::Response> {
        let valid_url = self.validate_url(request.url)?;
        let safe_url = sanitize_url_for_error(&valid_url);
        let mut req = self
            .request_builder(
                reqwest::Method::POST,
                &valid_url,
                DEFAULT_USER_AGENT,
                request.timeout_sec,
            )
            .header("Content-Type", "application/json")
            .json(request.body);
        for &(name, value) in request.headers {
            req = req.header(name, value);
        }
        req.send().await.map_err(|e| {
            let clean = e.without_url();
            AppError::Tool(format!("HTTP POST request failed for {safe_url}: {clean}"))
        })
    }

    pub(crate) async fn post_json(&self, request: PostJsonRequest<'_>) -> Result<HttpPostResponse> {
        self.post_json_until(request, |_, _| false).await
    }

    pub(crate) async fn post_json_until<F>(
        &self,
        request: PostJsonRequest<'_>,
        mut stop_when: F,
    ) -> Result<HttpPostResponse>
    where
        F: FnMut(&str, &[u8]) -> bool,
    {
        let resp = self.send_post_json(&request).await?;
        let status = resp.status();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = read_limited_until(resp, request.max_bytes, |buf| stop_when(&content_type, buf)).await?;
        let body = String::from_utf8_lossy(&bytes).to_string();
        Ok(HttpPostResponse {
            status,
            content_type,
            body,
        })
    }
}
