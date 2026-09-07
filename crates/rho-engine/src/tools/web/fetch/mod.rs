pub mod cache;
pub mod encoding;
pub mod extract;
pub mod pagination;

#[cfg(test)]
mod tests;

use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use crate::tools::web::http::{HttpClient, HttpRequest};
use cache::{CachedResource, FetchCache};
use pagination::{FormatPageParams, format_page};
pub use rho_harness_core::args::WebFetchArgs;
use rho_harness_core::error::AppError;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::sync::Arc;

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
    pub timeout_sec: u64,
    pub max_bytes: usize,
    pub pdf_max_bytes: usize,
    pub default_limit: usize,
}

struct FetchOptions<'a> {
    mode: &'a str,
    format_override: Option<&'a str>,
}

impl WebFetchTool {
    pub fn new(http: HttpClient, cache: FetchCache, config: WebFetchConfig) -> Self {
        Self {
            http,
            cache,
            timeout_sec: config.timeout_sec,
            max_bytes: config.max_bytes,
            pdf_max_bytes: config.pdf_max_bytes,
            default_limit: config.default_limit,
        }
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
        let limit = args.limit.unwrap_or(self.default_limit);
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
            timeout_sec: self.timeout_sec,
            max_bytes: self.max_bytes,
            pdf_max_bytes: Some(self.pdf_max_bytes),
        }
    }

    async fn fetch_and_extract(&self, url_str: &str, options: FetchOptions<'_>) -> Result<(String, String), AppError> {
        let resp = self.http.get_bytes(self.make_http_request(url_str)).await?;

        if encoding::is_pdf(&resp.body, &resp.content_type) || options.format_override == Some("pdf") {
            let text = extract::extract_pdf_bytes(resp.body).await?;
            return Ok((text, resp.final_url));
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

impl Tool for WebFetchTool {
    const NAME: &'static str = "web_fetch";
    type Args = WebFetchArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Fetch and extract readable content from a URL (HTML, JSON, Markdown, RSS/Atom, CSV, PDF).".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<WebFetchArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
