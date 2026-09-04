pub mod cache;
pub mod extract;
mod format;

#[cfg(test)]
mod tests;

use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use crate::tools::web::http::{HttpClient, HttpRequest};
use cache::FetchCache;
use format::{FormatFetchParams, format_fetch_output};
pub use rho_harness_core::args::WebFetchArgs;
use rho_harness_core::error::AppError;
use rig::tool::{Tool, ToolContext, ToolExecutionError};

pub struct WebFetchConfig {
    pub timeout_sec: u64,
    pub max_bytes: usize,
    pub default_limit: usize,
}

#[derive(Clone)]
pub struct WebFetchTool {
    pub http: HttpClient,
    pub cache: FetchCache,
    pub timeout_sec: u64,
    pub max_bytes: usize,
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
            default_limit: config.default_limit,
        }
    }

    pub async fn execute(&self, args: WebFetchArgs) -> Result<ToolResult, AppError> {
        let url_str = args.url.trim();
        if url_str.is_empty() {
            return Ok(ToolResult::error("Empty URL provided for fetch"));
        }

        let mode = args.mode.unwrap_or_else(|| "auto".to_string());
        let offset = args.offset.unwrap_or(1).max(1);
        let limit = args.limit.unwrap_or(self.default_limit);

        let cache_key = format!("{}:{}:{}", url_str, mode, args.format.as_deref().unwrap_or(""));

        let full_text = if let Some(cached) = self.cache.get(&cache_key).await {
            cached
        } else {
            let options = FetchOptions {
                mode: &mode,
                format_override: args.format.as_deref(),
            };
            let extracted = self.fetch_and_extract(url_str, options).await?;
            self.cache.insert(cache_key, extracted.clone()).await;
            extracted
        };

        Ok(format_fetch_output(FormatFetchParams {
            text: &full_text,
            offset,
            limit,
            url_str,
        }))
    }

    async fn fetch_and_extract(&self, url_str: &str, options: FetchOptions<'_>) -> Result<String, AppError> {
        if extract::is_pdf_request(url_str, options.format_override) {
            let (bytes, _) = self
                .http
                .get_bytes(HttpRequest {
                    url: url_str,
                    user_agent: None,
                    timeout_sec: self.timeout_sec,
                    max_bytes: self.max_bytes,
                })
                .await?;
            return extract::extract_pdf_bytes(bytes).await;
        }

        let (body, content_type) = self
            .http
            .get_text(HttpRequest {
                url: url_str,
                user_agent: None,
                timeout_sec: self.timeout_sec,
                max_bytes: self.max_bytes,
            })
            .await?;

        Ok(extract::extract_text(extract::ExtractTextParams {
            body: &body,
            content_type: &content_type,
            url_str,
            mode: options.mode,
            format_override: options.format_override,
        }))
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
