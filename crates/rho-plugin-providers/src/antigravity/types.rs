use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityTokens {
    #[serde(alias = "access")]
    pub access_token: String,
    #[serde(alias = "refresh")]
    pub refresh_token: String,
    #[serde(alias = "expires")]
    pub expires_at: i64,
    #[serde(alias = "projectId", skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "thought", skip_serializing_if = "Option::is_none")]
    pub thought: Option<bool>,
    #[serde(
        rename = "thoughtSignature",
        alias = "thought_signature",
        skip_serializing_if = "Option::is_none"
    )]
    pub thought_signature: Option<String>,
    #[serde(rename = "inlineData", skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<GeminiInlineData>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    pub function_call: Option<GeminiFunctionCall>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    pub function_response: Option<GeminiFunctionResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiInlineData {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiFunctionCall {
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiFunctionResponse {
    pub name: String,
    pub response: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiContent {
    pub role: String,
    pub parts: Vec<GeminiPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiSystemInstruction {
    pub role: String,
    pub parts: Vec<GeminiTextPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiTextPart {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(rename = "maxOutputTokens", skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(rename = "thinkingConfig", skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<GeminiThinkingConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GeminiThinkingConfig {
    #[serde(rename = "includeThoughts", skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,
    #[serde(rename = "thinkingLevel", skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    #[serde(rename = "thinkingBudget", skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiFunctionDeclaration {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "parametersJsonSchema", skip_serializing_if = "Option::is_none")]
    pub parameters_json_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiTools {
    #[serde(rename = "functionDeclarations")]
    pub function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityRequestBody {
    pub contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiSystemInstruction>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiTools>>,
    #[serde(rename = "toolConfig", skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<Value>,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityGenerateRequest {
    pub project: String,
    pub model: String,
    pub request: AntigravityRequestBody,
    #[serde(rename = "requestType")]
    pub request_type: String,
    #[serde(rename = "userAgent")]
    pub user_agent: String,
    #[serde(rename = "requestId")]
    pub request_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChunkCandidate {
    pub content: Option<GeminiContent>,
    #[serde(rename = "finishReason")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChunkUsage {
    #[serde(rename = "promptTokenCount")]
    pub prompt_tokens: Option<u64>,
    #[serde(rename = "candidatesTokenCount")]
    pub candidates_tokens: Option<u64>,
    #[serde(rename = "totalTokenCount")]
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChunkResponse {
    pub response: Option<StreamResponsePayload>,
    pub candidates: Option<Vec<StreamChunkCandidate>>,
    #[serde(rename = "usageMetadata")]
    pub usage_metadata: Option<StreamChunkUsage>,
    pub error: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamResponsePayload {
    pub candidates: Option<Vec<StreamChunkCandidate>>,
    #[serde(rename = "usageMetadata")]
    pub usage_metadata: Option<StreamChunkUsage>,
}
