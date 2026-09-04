//! JWT payload parsing helpers for OAuth tokens.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};

/// Extracts the `chatgpt_account_id` claim from a ChatGPT OAuth JWT.
pub fn extract_chatgpt_account_id(jwt: &str) -> Option<String> {
    let payload_b64 = jwt.split('.').nth(1)?;
    let mut padded = payload_b64.to_string();
    let rem = padded.len() % 4;
    if rem > 0 {
        padded.push_str(&"=".repeat(4 - rem));
    }

    let decoded = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .or_else(|_| URL_SAFE.decode(&padded))
        .or_else(|_| STANDARD_NO_PAD.decode(payload_b64))
        .or_else(|_| STANDARD.decode(&padded))
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    value
        .get("https://api.openai.com/auth")
        .and_then(|v| v.get("chatgpt_account_id"))
        .and_then(|v| v.as_str())
        .map(String::from)
}
