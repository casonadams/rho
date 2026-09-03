use rho_harness_core::provider::ProviderId;
use std::str::FromStr;

/// Whether a provider's rig adapter serializes `ToolResultContent::Image`
/// blocks inside tool results.
///
/// rig 0.42 splits at the provider boundary: the Anthropic and Gemini adapters
/// map tool-result image blocks, and the ChatGpt (Codex) client rides the
/// Responses API, which accepts them. The OpenAI-compatible completions
/// adapters (openai, openrouter, ollama, xai, groq, deepseek, mistral, cohere,
/// copilot, antigravity) hard-error with "does not support images in tool
/// results", failing the whole request. Unknown providers default to false:
/// omitting an image beats failing the request.
pub fn supports_tool_result_images(provider: &str) -> bool {
    matches!(
        ProviderId::from_str(provider),
        Ok(ProviderId::Anthropic | ProviderId::Gemini | ProviderId::ChatGpt)
    )
}

#[cfg(test)]
mod tests;
