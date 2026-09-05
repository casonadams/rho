//! Ollama cloud service (ollama.com) account quota display.

#[cfg(test)]
mod tests;

use serde_json::Value;
use std::sync::LazyLock;
use std::time::Duration;

pub const CLOUD_HOST: &str = "https://ollama.com";

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    crate::install_crypto_provider();
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default()
});

/// Fetch the account usage summary from ollama.com and format it for the status line.
pub async fn fetch_quota(api_key: &str) -> Option<String> {
    let response = CLIENT
        .get(format!("{CLOUD_HOST}/api/usage"))
        .bearer_auth(api_key.trim())
        .send()
        .await
        .ok()?
        .json::<Value>()
        .await
        .ok()?;
    parse_quota(&response)
}

/// Ollama reports `limits.monthly.usage` as the fraction (0..1) of the monthly
/// quota already consumed; no reset timestamp is included in the payload.
pub fn parse_quota(value: &Value) -> Option<String> {
    let usage = value.get("limits")?.get("monthly")?.get("usage")?.as_f64()?;
    let pct = (usage * 100.0).round().clamp(0.0, 100.0) as u64;
    Some(format!("{pct}% used"))
}
