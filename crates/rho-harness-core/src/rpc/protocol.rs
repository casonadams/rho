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
    SetThinking {
        level: String,
    },
    GetTree,
    SwitchBranch {
        node_id: String,
    },
    SetNodeLabel {
        node_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    ListSessions,
    ResumeSession {
        session_id: String,
    },
    ForkSession {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node_id: Option<String>,
    },
    GetState,
    GetNodeInfo,
    CreateSession {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace: Option<String>,
    },
    AuthLogin {
        provider: String,
    },
    AuthInput {
        interaction_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secret_value: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selected_option: Option<String>,
    },
    SetApiKey {
        provider: String,
        api_key: String,
    },
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
    StatusChanged {
        status: String,
    },
    NodeInfo {
        hostname: String,
        os: String,
        arch: String,
        version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        active_workspace: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        active_branch: Option<String>,
        status: String,
    },
    AuthRequest {
        interaction_id: String,
        provider: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_code: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_secret: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<Vec<crate::auth::SelectOption>>,
    },
    AuthComplete {
        provider: String,
        success: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
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
    }

    #[test]
    fn test_rpc_response_serialization() {
        let res = RpcResponse::success(Some("req-1".to_string()), "prompt", None);
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

    #[test]
    fn test_rpc_extended_commands_parsing() {
        let set_thinking: RpcRequest = serde_json::from_str(r#"{"type":"set_thinking","level":"high"}"#).unwrap();
        assert_eq!(
            set_thinking.command,
            RpcCommand::SetThinking {
                level: "high".to_string()
            }
        );

        let get_tree: RpcRequest = serde_json::from_str(r#"{"type":"get_tree"}"#).unwrap();
        assert_eq!(get_tree.command, RpcCommand::GetTree);

        let switch_branch: RpcRequest =
            serde_json::from_str(r#"{"type":"switch_branch","node_id":"node-123"}"#).unwrap();
        assert_eq!(
            switch_branch.command,
            RpcCommand::SwitchBranch {
                node_id: "node-123".to_string()
            }
        );

        let set_label: RpcRequest =
            serde_json::from_str(r#"{"type":"set_node_label","node_id":"node-1","label":"v1.0"}"#).unwrap();
        assert_eq!(
            set_label.command,
            RpcCommand::SetNodeLabel {
                node_id: "node-1".to_string(),
                label: Some("v1.0".to_string())
            }
        );

        let list_sessions: RpcRequest = serde_json::from_str(r#"{"type":"list_sessions"}"#).unwrap();
        assert_eq!(list_sessions.command, RpcCommand::ListSessions);

        let resume: RpcRequest = serde_json::from_str(r#"{"type":"resume_session","session_id":"sess-abc"}"#).unwrap();
        assert_eq!(
            resume.command,
            RpcCommand::ResumeSession {
                session_id: "sess-abc".to_string()
            }
        );

        let fork: RpcRequest = serde_json::from_str(r#"{"type":"fork_session"}"#).unwrap();
        assert_eq!(fork.command, RpcCommand::ForkSession { node_id: None });

        let get_info: RpcRequest = serde_json::from_str(r#"{"type":"get_node_info"}"#).unwrap();
        assert_eq!(get_info.command, RpcCommand::GetNodeInfo);

        let create_session: RpcRequest =
            serde_json::from_str(r#"{"type":"create_session","workspace":"/tmp/repo"}"#).unwrap();
        assert_eq!(
            create_session.command,
            RpcCommand::CreateSession {
                workspace: Some("/tmp/repo".to_string())
            }
        );

        let auth_login: RpcRequest = serde_json::from_str(r#"{"type":"auth_login","provider":"antigravity"}"#).unwrap();
        assert_eq!(
            auth_login.command,
            RpcCommand::AuthLogin {
                provider: "antigravity".to_string()
            }
        );

        let auth_input: RpcRequest =
            serde_json::from_str(r#"{"type":"auth_input","interaction_id":"int-1","secret_value":"tok-123"}"#).unwrap();
        assert_eq!(
            auth_input.command,
            RpcCommand::AuthInput {
                interaction_id: "int-1".to_string(),
                secret_value: Some("tok-123".to_string()),
                selected_option: None,
            }
        );

        let set_key: RpcRequest =
            serde_json::from_str(r#"{"type":"set_api_key","provider":"anthropic","api_key":"sk-ant-test"}"#).unwrap();
        assert_eq!(
            set_key.command,
            RpcCommand::SetApiKey {
                provider: "anthropic".to_string(),
                api_key: "sk-ant-test".to_string(),
            }
        );
    }

    #[test]
    fn test_rpc_node_info_and_auth_events() {
        let node_info = RpcEvent::NodeInfo {
            hostname: "mbp".to_string(),
            os: "macos".to_string(),
            arch: "aarch64".to_string(),
            version: "0.7.1".to_string(),
            active_workspace: Some("/Users/test/repo".to_string()),
            active_branch: Some("main".to_string()),
            status: "idle".to_string(),
        };
        let node_json = serde_json::to_string(&node_info).unwrap();
        assert!(node_json.contains("\"type\":\"node_info\""));
        assert!(node_json.contains("\"hostname\":\"mbp\""));

        let auth_req = RpcEvent::AuthRequest {
            interaction_id: "req-1".to_string(),
            provider: "claude".to_string(),
            auth_url: Some("https://claude.ai/oauth".to_string()),
            instructions: None,
            user_code: None,
            prompt: None,
            is_secret: None,
            options: None,
        };
        let auth_json = serde_json::to_string(&auth_req).unwrap();
        assert!(auth_json.contains("\"type\":\"auth_request\""));
        assert!(auth_json.contains("\"provider\":\"claude\""));

        let auth_comp = RpcEvent::AuthComplete {
            provider: "claude".to_string(),
            success: true,
            error: None,
        };
        let comp_json = serde_json::to_string(&auth_comp).unwrap();
        assert_eq!(
            comp_json,
            r#"{"type":"auth_complete","provider":"claude","success":true}"#
        );
    }

    #[test]
    fn test_rpc_status_changed_event() {
        let event = RpcEvent::StatusChanged {
            status: "waiting_approval".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, r#"{"type":"status_changed","status":"waiting_approval"}"#);
    }
}
