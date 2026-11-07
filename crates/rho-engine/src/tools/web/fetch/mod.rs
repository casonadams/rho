pub mod cache;
pub mod encoding;
pub mod extract;
pub mod pagination;

#[cfg(test)]
mod tests;

use crate::tools::engine_tool::EngineTool;
use crate::tools::types::{ToolResult, generated_schema};
use crate::tools::web::http::{HttpClient, HttpRequest};
use cache::{CachedResource, FetchCache};
use pagination::{FormatPageParams, format_page};
pub use rho_harness_core::args::WebFetchArgs;
use rho_harness_core::error::AppError;
use std::sync::Arc;

#[derive(Clone)]
pub struct WebFetchConfig {
    pub timeout_sec: u64,
    pub max_bytes: usize,
    pub pdf_max_bytes: usize,
    pub default_limit: usize,
}

#[derive(Clone)]
pub struct WebFetchTool {
    pub http: HttpClient,
    pub cache: FetchCache,
    pub config: WebFetchConfig,
}

struct FetchOptions<'a> {
    mode: &'a str,
    format_override: Option<&'a str>,
}

impl WebFetchTool {
    pub fn new(http: HttpClient, cache: FetchCache, config: WebFetchConfig) -> Self {
        Self { http, cache, config }
    }

    async fn get_or_fetch_resource(
        &self,
        url_str: &str,
        options: FetchOptions<'_>,
    ) -> Result<CachedResource, AppError> {
        let cache_key = format!("{url_str}:{}:{}", options.mode, options.format_override.unwrap_or(""));
        if let Some(cached) = self.cache.get(&cache_key).await {
            return Ok(cached);
        }
        let (text, final_url) = self.fetch_and_extract(url_str, options).await?;
        let res = CachedResource {
            text: Arc::from(text),
            final_url: Arc::from(final_url),
        };
        self.cache.insert(cache_key, res.clone()).await;
        Ok(res)
    }

    pub async fn execute(&self, args: WebFetchArgs) -> Result<ToolResult, AppError> {
        let url_str = args.url.trim();
        if url_str.is_empty() {
            return Ok(ToolResult::error("Empty URL provided for fetch"));
        }

        let mode = args.mode.unwrap_or_else(|| "auto".to_string());
        let offset = args.offset.unwrap_or(1).max(1);
        let limit = args.limit.unwrap_or(self.config.default_limit);
        let options = FetchOptions {
            mode: &mode,
            format_override: args.format.as_deref(),
        };

        let resource = match self.get_or_fetch_resource(url_str, options).await {
            Ok(r) => r,
            Err(err) => return Ok(ToolResult::error(err.to_string())),
        };

        Ok(format_page(FormatPageParams {
            text: &resource.text,
            offset,
            limit,
            source_url: url_str,
            final_url: &resource.final_url,
        }))
    }

    fn make_http_request<'a>(&self, url: &'a str) -> HttpRequest<'a> {
        HttpRequest {
            url,
            user_agent: None,
            timeout_sec: self.config.timeout_sec,
            max_bytes: self.config.max_bytes,
            pdf_max_bytes: Some(self.config.pdf_max_bytes),
        }
    }

    async fn fetch_and_extract(&self, url_str: &str, options: FetchOptions<'_>) -> Result<(String, String), AppError> {
        let resp = self.http.get_bytes(self.make_http_request(url_str)).await?;

        if encoding::is_pdf(&resp.body, &resp.content_type) || options.format_override == Some("pdf") {
            let text = extract::extract_pdf_bytes(resp.body).await?;
            return Ok((text, resp.final_url));
        }

        if encoding::is_image(&resp.body, &resp.content_type, &resp.final_url) {
            let mime = encoding::image_mime_type(&resp.body, &resp.content_type, &resp.final_url)
                .unwrap_or(resp.content_type.as_str());
            return Err(AppError::Tool(format!(
                "Direct image content ({mime}) cannot be parsed as text"
            )));
        }

        let decoded = encoding::decode_body(&resp.body, &resp.content_type);
        let text = extract::extract_text(extract::ExtractTextParams {
            body: &decoded,
            content_type: &resp.content_type,
            url_str: &resp.final_url,
            mode: options.mode,
            format_override: options.format_override,
        })?;

        Ok((text, resp.final_url))
    }
}

#[async_trait::async_trait]
impl EngineTool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch and extract readable content from a URL (HTML, JSON, Markdown, RSS/Atom, CSV, PDF)."
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<WebFetchArgs>()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, AppError> {
        let args: WebFetchArgs = match serde_json::from_value(args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::error(format!("failed to parse tool arguments: {e}"))),
        };
        self.execute(args).await
    }
}
