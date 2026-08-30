use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The complete question to ask the user. Should be clear, specific, and end
/// with a question mark.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct AskUserArgs {
    #[serde(default)]
    pub question: Option<String>,
    /// The available choices for this question (2-4 options recommended).
    #[serde(default)]
    pub options: Option<Vec<Value>>,
    /// Very short chip/tag shown next to the question (1-3 words).
    #[serde(default)]
    pub header: Option<String>,
    /// List of multiple questions to ask in a single prompt sequence.
    #[serde(default)]
    pub questions: Option<Vec<Value>>,
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, Value>,
}
