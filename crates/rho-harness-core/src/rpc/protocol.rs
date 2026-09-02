use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcCommand {
    Prompt {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<Vec<Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        streaming_behavior: Option<String>,
    },
    Steer {
        message: String,
    },
    Abort,
    ToolResponse {
        approval_id: String,
        decision: String,
    },
    Compact {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
    },
    SetModel {
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
    },
    GetState,
    Exit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(flatten)]
    pub command: RpcCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub r#type: String,
    pub command: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RpcResponse {
    pub fn success(id: Option<String>, command: &str, data: Option<Value>) -> Self {
        Self {
            id,
            r#type: "response".to_string(),
            command: command.to_string(),
            success: true,
            data,
            error: None,
        }
    }

    pub fn failure(id: Option<String>, command: &str, error: &str) -> Self {
        Self {
            id,
            r#type: "response".to_string(),
            command: command.to_string(),
            success: false,
            data: None,
            error: Some(error.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcEvent {
    SessionStart {
        session_id: String,
        model: String,
        provider: String,
    },
    TurnStart {
        turn_number: usize,
        prompt: String,
    },
    TextChunk {
        content: String,
    },
    ReasoningChunk {
        content: String,
    },
    ToolCallStart {
        call_id: String,
        tool: String,
        arguments: Value,
    },
    ToolApprovalRequest {
        approval_id: String,
        tool: String,
        arguments: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    ToolCallResult {
        call_id: String,
        tool: String,
        output: String,
        is_error: bool,
        duration_ms: u64,
    },
    UsageUpdate {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_percent: Option<f64>,
    },
    TurnEnd {
        stop_reason: String,
    },
    Error {
        code: String,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_request_parsing() {
        let json = r#"{"id":"req-1","type":"prompt","message":"hello"}"#;
        let req: RpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.id, Some("req-1".to_string()));
        assert!(matches!(req.command, RpcCommand::Prompt { message, .. } if message == "hello"));

        let res = RpcResponse::success(req.id, "prompt", None);
        let res_json = serde_json::to_string(&res).unwrap();
        assert!(res_json.contains("\"success\":true"));
        assert!(res_json.contains("\"id\":\"req-1\""));
    }

    #[test]
    fn test_rpc_event_serialization() {
        let event = RpcEvent::TextChunk {
            content: "Hello world".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, r#"{"type":"text_chunk","content":"Hello world"}"#);
    }
}
