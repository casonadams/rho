pub mod cache;
pub mod extract;

use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use crate::tools::web::http::{HttpClient, HttpRequest};
use cache::FetchCache;
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
            let extracted = self.fetch_and_extract(url_str, (&mode, args.format.as_deref())).await?;
            self.cache.insert(cache_key, extracted.clone()).await;
            extracted
        };

        let lines: Vec<&str> = full_text.lines().collect();
        let total_lines = lines.len();

        if total_lines == 0 {
            return Ok(ToolResult::success("[Empty content returned from URL]"));
        }

        let start_idx = (offset - 1).min(total_lines);
        let end_idx = (start_idx + limit).min(total_lines);

        let mut output = String::new();
        for (i, line) in lines[start_idx..end_idx].iter().enumerate() {
            let line_num = start_idx + i + 1;
            output.push_str(&format!("{line_num:5}\t{line}\n"));
        }

        if end_idx < total_lines {
            output.push_str(&format!(
                "\n[Lines {}-{} of {} total lines from {}]",
                offset, end_idx, total_lines, url_str
            ));
        }

        Ok(ToolResult::success(output))
    }

    async fn fetch_and_extract(&self, url_str: &str, options: (&str, Option<&str>)) -> Result<String, AppError> {
        let (mode, format_override) = options;
        // PDF check
        let is_pdf = format_override == Some("pdf")
            || url_str.to_lowercase().ends_with(".pdf")
            || url_str.to_lowercase().contains(".pdf?");

        if is_pdf {
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
        let ct_lower = content_type.to_lowercase();

        if let Some(fmt) = format_override {
            match fmt.to_lowercase().as_str() {
                "json" => return Ok(extract::extract_json(&body)),
                "csv" | "tsv" => return Ok(extract::extract_csv(&body, if fmt == "tsv" { b'\t' } else { b',' })),
                "xml" | "rss" | "atom" => return Ok(extract::extract_feed_or_xml(&body, url_str)),
                "markdown" | "md" => return Ok(extract::resolve_markdown_links(&body, url_str)),
                _ => {}
            }
        }

        if ct_lower.contains("json") {
            Ok(extract::extract_json(&body))
        } else if ct_lower.contains("xml") || ct_lower.contains("rss") || ct_lower.contains("atom") {
            Ok(extract::extract_feed_or_xml(&body, url_str))
        } else if ct_lower.contains("csv") || ct_lower.contains("tab-separated") {
            let delim = if ct_lower.contains("tab-separated") || url_str.ends_with(".tsv") {
                b'\t'
            } else {
                b','
            };
            Ok(extract::extract_csv(&body, delim))
        } else if ct_lower.contains("markdown") || url_str.ends_with(".md") {
            Ok(extract::resolve_markdown_links(&body, url_str))
        } else {
            // Default HTML extraction
            Ok(extract::extract_html(&body, url_str, mode))
        }
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
