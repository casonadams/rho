use futures::StreamExt;
use rho_harness_core::error::{AppError, Result};

pub struct ReadLimitedParams<'a> {
    pub response: reqwest::Response,
    pub content_type: &'a str,
    pub max_bytes: usize,
    pub pdf_max_bytes: Option<usize>,
}

fn determine_limit(content_type: &str, max_bytes: usize, pdf_max_bytes: Option<usize>) -> usize {
    if content_type.to_lowercase().contains("application/pdf")
        && let Some(pdf_limit) = pdf_max_bytes
    {
        pdf_limit
    } else {
        max_bytes
    }
}

fn check_declared_length(response: &reqwest::Response, limit: usize) -> Result<()> {
    if let Some(cl) = response.content_length()
        && cl as usize > limit
    {
        return Err(AppError::Tool(format!(
            "Response is too large ({cl} bytes; limit is {limit})"
        )));
    }
    Ok(())
}

pub async fn read_limited(params: ReadLimitedParams<'_>) -> Result<Vec<u8>> {
    let mut limit = determine_limit(params.content_type, params.max_bytes, params.pdf_max_bytes);
    check_declared_length(&params.response, limit)?;

    let mut body: Vec<u8> = Vec::with_capacity(limit.min(256 * 1024));
    let mut stream = params.response.bytes_stream();
    let mut checked_pdf = false;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::Tool(format!("Network read error: {e}")))?;
        if !checked_pdf && body.is_empty() {
            if chunk.starts_with(b"%PDF-")
                && let Some(pdf_limit) = params.pdf_max_bytes
            {
                limit = pdf_limit;
            }
            checked_pdf = true;
        }
        if body.len() + chunk.len() > limit {
            return Err(AppError::Tool(format!("Response exceeded the {limit}-byte limit")));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
