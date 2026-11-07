//! Shared send/status-check/decode path for OAuth token endpoints.

use crate::http::CLIENT;
use rho_harness_core::error::{AppError, Result};
use serde::de::DeserializeOwned;
use std::time::Duration;

pub(crate) async fn send_checked(request: reqwest::RequestBuilder, operation: &str) -> Result<reqwest::Response> {
    let res = request
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("{operation} request failed: {e}")))?;
    if res.status().is_success() {
        return Ok(res);
    }
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    Err(AppError::Auth(format!("{operation} failed ({status}): {body}")))
}

pub(crate) async fn send_for_json<T: DeserializeOwned>(request: reqwest::RequestBuilder, operation: &str) -> Result<T> {
    send_checked(request, operation)
        .await?
        .json()
        .await
        .map_err(|e| AppError::Auth(format!("Failed to parse {operation} response: {e}")))
}

pub(crate) async fn post_form_json<T: DeserializeOwned>(
    url: &str,
    form: &[(&str, &str)],
    headers: &[(&str, &str)],
    timeout: Duration,
    operation: &str,
) -> Result<T> {
    let mut request = CLIENT.post(url).timeout(timeout).form(form);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    send_for_json(request, operation).await
}
