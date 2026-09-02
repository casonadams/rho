use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum StepEvent {
    CompletionCall {
        turn: usize,
        prompt: Value,
        history: Vec<Value>,
    },
    CompletionResponse {
        prompt: Value,
        response: Value,
    },
    ToolCall {
        tool_name: String,
        args: Value,
    },
    ToolResult {
        tool_name: String,
        args: Value,
        output: String,
        is_error: bool,
    },
    InvalidToolCall {
        tool_name: String,
        args: Value,
        available_tools: Vec<String>,
    },
    TextDelta {
        delta: String,
    },
    ReasoningDelta {
        delta: String,
    },
    TurnStart {
        prompt: String,
    },
    TurnEnd {
        status: String,
        tool_calls_count: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub text: String,
}

impl Document {
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RequestPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preamble: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_params: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_context: Option<Vec<Document>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Flow {
    Continue,
    Skip { reason: String },
    RewriteArgs { args: Value },
    RewriteResult { result: String },
    OverrideRequest { request: RequestPatch },
    Repair { tool_name: String },
    Retry { feedback: String },
    Terminate { reason: String },
}

impl Flow {
    pub fn cont() -> Self {
        Self::Continue
    }

    pub fn skip(reason: impl Into<String>) -> Self {
        Self::Skip { reason: reason.into() }
    }

    pub fn rewrite_args(args: impl Into<Value>) -> Self {
        Self::RewriteArgs { args: args.into() }
    }

    pub fn rewrite_result(result: impl Into<String>) -> Self {
        Self::RewriteResult { result: result.into() }
    }

    pub fn repair(tool_name: impl Into<String>) -> Self {
        Self::Repair {
            tool_name: tool_name.into(),
        }
    }

    pub fn retry(feedback: impl Into<String>) -> Self {
        Self::Retry {
            feedback: feedback.into(),
        }
    }

    pub fn override_request(request: RequestPatch) -> Self {
        Self::OverrideRequest { request }
    }

    pub fn terminate(reason: impl Into<String>) -> Self {
        Self::Terminate { reason: reason.into() }
    }
}
