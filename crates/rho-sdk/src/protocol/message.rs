use super::envelope::{RequestId, StructuredError};
use crate::capability::{CapabilityId, CapabilityManifest};
use crate::contract::{
    AuthenticationRequest, AuthenticationResponse, CapabilityDescriptor, CommandInvocationRequest,
    CommandInvocationResponse, ContextRequest, ContextResponse, LifecycleEvent, PermissionDecision, ProviderRequest,
    ProviderStreamEvent, RequestedOperation, SkillAsset, ToolInvocationRequest, ToolInvocationResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolMessage {
    HandshakeRequest {
        supported_versions: Vec<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plugin_config: Option<Value>,
    },
    DiscoveryRequest,
    InvocationRequest {
        capability_id: CapabilityId,
        invocation: InvocationRequest,
    },
    StreamEvent {
        event: StreamEvent,
    },
    CancelRequest {
        target_request_id: RequestId,
    },
    TerminalResponse {
        result: TerminalResult,
    },
    ErrorResponse {
        error: StructuredError,
    },
}

impl ProtocolMessage {
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::TerminalResponse { .. } | Self::ErrorResponse { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "request", rename_all = "snake_case")]
pub enum InvocationRequest {
    ProviderStream(ProviderRequest),
    ProviderAuthenticate(AuthenticationRequest),
    Tool(ToolInvocationRequest),
    Permission(RequestedOperation),
    Command(CommandInvocationRequest),
    Lifecycle(LifecycleEvent),
    Skills,
    Context(ContextRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "event", rename_all = "snake_case")]
pub enum StreamEvent {
    Provider(ProviderStreamEvent),
    Progress { message: String },
    CommandOutput { content: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "response", rename_all = "snake_case")]
pub enum TerminalResult {
    Handshake {
        selected_version: u32,
    },
    Discovery {
        manifest: CapabilityManifest,
        capabilities: Vec<CapabilityDescriptor>,
    },
    ProviderAuthenticated(AuthenticationResponse),
    Tool(ToolInvocationResponse),
    Permission(PermissionDecision),
    Command(CommandInvocationResponse),
    Lifecycle,
    Skills(Vec<SkillAsset>),
    Context(ContextResponse),
    StreamCompleted,
    Cancelled,
}
