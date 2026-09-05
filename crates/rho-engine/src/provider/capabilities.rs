use rho_harness_core::provider::ProviderId;
use std::str::FromStr;

/// Whether a provider's rig adapter serializes `ToolResultContent::Image`
/// blocks inside tool results.
///
/// rig 0.42 splits at the provider boundary: the Anthropic and Gemini adapters
/// map tool-result image blocks, the ChatGpt client rides the Responses API,
/// and Antigravity formats them via Gemini-shaped function responses. The
/// OpenAI-compatible completions adapters (openai, openrouter, ollama, xai,
/// groq, deepseek, mistral, cohere, copilot) hard-error with "does not support
/// images in tool results". Unknown providers default to false.
pub fn supports_tool_result_images(provider: &str) -> bool {
    matches!(
        ProviderId::from_str(provider),
        Ok(ProviderId::Anthropic
            | ProviderId::Gemini
            | ProviderId::ChatGpt
            | ProviderId::Antigravity
            | ProviderId::ClaudeCode,)
    )
}

#[cfg(test)]
mod tests;
