pub mod cache;
pub mod encoding;
pub mod extract;
pub mod multimodal;
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
    pub multimodal: bool,
    pub auth_file: Option<std::path::PathBuf>,
}

#[derive(Clone)]
pub struct WebFetchTool {
    pub http: HttpClient,
    pub cache: FetchCache,
    pub timeout_sec: u64,
    pub max_bytes: usize,
    pub pdf_max_bytes: usize,
    pub default_limit: usize,
    pub multimodal: bool,
    pub auth_file: Option<std::path::PathBuf>,
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
            multimodal: config.multimodal,
            auth_file: config.auth_file,
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

    async fn try_specialized_extract(
        &self,
        url_str: &str,
        format_override: Option<&str>,
    ) -> Option<Result<(String, String), AppError>> {
        if format_override == Some("html") {
            return None;
        }

        if let Some(github_url) = extract::parse_github_url(url_str) {
            match extract::extract_github(&self.http, &github_url, self.timeout_sec).await {
                Ok(text) => return Some(Ok((text, url_str.to_string()))),
                Err(err) if err.to_string().contains("rate limit") => return Some(Err(err)),
                Err(_) => {}
            }
        }

        if let Some(youtube_url) = extract::parse_youtube_url(url_str)
            && let Ok(text) = extract::extract_youtube(&self.http, &youtube_url, self.timeout_sec).await
        {
            let final_url = format!("https://www.youtube.com/watch?v={}", youtube_url.video_id);
            return Some(Ok((text, final_url)));
        }

        None
    }

    async fn fallback_pdf(&self, bytes: &[u8], native_err: AppError) -> Result<String, AppError> {
        if !self.multimodal {
            return Err(native_err);
        }
        let payload = multimodal::build_pdf_payload(bytes)?;
        multimodal::analyze_multimodal(&self.http, self.auth_file.as_deref(), payload, self.timeout_sec)
            .await
            .map_err(|gemini_err| {
                AppError::Tool(format!(
                    "PDF text extraction failed ({native_err}) and Gemini multimodal fallback failed ({gemini_err})"
                ))
            })
    }

    async fn extract_pdf(&self, bytes: &[u8], force_multimodal: bool) -> Result<String, AppError> {
        if force_multimodal {
            if !self.multimodal {
                return Err(AppError::Tool(
                    "Multimodal extraction requested but tools.web.fetch.multimodal is disabled in configuration"
                        .to_string(),
                ));
            }
            let payload = multimodal::build_pdf_payload(bytes)?;
            return multimodal::analyze_multimodal(&self.http, self.auth_file.as_deref(), payload, self.timeout_sec)
                .await;
        }

        match extract::extract_pdf_bytes(bytes.to_vec()).await {
            Ok(text) if self.multimodal && multimodal::is_low_confidence_pdf(&text, bytes.len()) => {
                let payload = multimodal::build_pdf_payload(bytes)?;
                multimodal::analyze_multimodal(&self.http, self.auth_file.as_deref(), payload, self.timeout_sec)
                    .await
                    .or(Ok(text))
            }
            Ok(text) => Ok(text),
            Err(err) => self.fallback_pdf(bytes, err).await,
        }
    }

    async fn extract_image(&self, bytes: &[u8], content_type: &str, url: &str) -> Result<String, AppError> {
        if !self.multimodal {
            let mime = encoding::image_mime_type(bytes, content_type, url).unwrap_or(content_type);
            return Err(AppError::Tool(format!(
                "Direct image content ({mime}) cannot be parsed as text because tools.web.fetch.multimodal is disabled"
            )));
        }

        let mime = encoding::image_mime_type(bytes, content_type, url).unwrap_or(content_type);
        let payload = if mime == "image/svg+xml" {
            let svg_text = String::from_utf8_lossy(bytes);
            multimodal::build_svg_payload(&svg_text)?
        } else {
            multimodal::build_image_payload(bytes, mime)?
        };

        multimodal::analyze_multimodal(&self.http, self.auth_file.as_deref(), payload, self.timeout_sec).await
    }

    async fn fetch_and_extract(&self, url_str: &str, options: FetchOptions<'_>) -> Result<(String, String), AppError> {
        if let Some(result) = self.try_specialized_extract(url_str, options.format_override).await {
            return result;
        }

        let resp = self.http.get_bytes(self.make_http_request(url_str)).await?;
        let force_multimodal =
            options.format_override == Some("multimodal") || options.format_override == Some("image");

        if encoding::is_pdf(&resp.body, &resp.content_type) || options.format_override == Some("pdf") {
            let text = self.extract_pdf(&resp.body, force_multimodal).await?;
            return Ok((text, resp.final_url));
        }

        if encoding::is_image(&resp.body, &resp.content_type, &resp.final_url) || force_multimodal {
            let text = self
                .extract_image(&resp.body, &resp.content_type, &resp.final_url)
                .await?;
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
