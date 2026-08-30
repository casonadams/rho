use crate::capability::{CapabilityError, CapabilityId, CapabilityKind, CapabilityValidationError};
use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::{Debug, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    pub id: CapabilityId,
    pub display_name: String,
    pub models: Vec<ModelMetadata>,
    pub authentication: Vec<AuthenticationMethod>,
}

impl ProviderDescriptor {
    pub fn validate(&self) -> Result<(), ContractValidationError> {
        self.id.require_kind(CapabilityKind::Provider)?;
        require_text("provider display name", &self.display_name)?;
        let mut model_ids = std::collections::BTreeSet::new();
        for model in &self.models {
            require_text("model identifier", &model.id)?;
            require_text("model display name", &model.display_name)?;
            if !model_ids.insert(&model.id) {
                return Err(ContractValidationError::DuplicateModel(model.id.clone()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_limit: Option<u64>,
    pub supports_tools: bool,
    pub supports_images: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthenticationMethod {
    None,
    ApiKey { label: String },
    OAuth { label: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticationRequest {
    pub operation: AuthenticationOperation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<ScopedCredential>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationOperation {
    Login,
    Refresh,
    Verify,
    Logout,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedCredential {
    pub kind: String,
    pub value: Value,
}

impl Debug for ScopedCredential {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScopedCredential")
            .field("kind", &self.kind)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticationResponse {
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshed_credential: Option<ScopedCredential>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<ModelMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<ScopedCredential>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    pub tools: Vec<ProviderToolDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: MessageRole,
    pub content: Vec<MessageContent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    Text {
        text: String,
    },
    ToolCall {
        call_id: String,
        tool_id: CapabilityId,
        arguments: Value,
    },
    ToolResult {
        call_id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderToolDefinition {
    pub id: CapabilityId,
    pub description: String,
    pub argument_schema: Value,
}

impl ProviderToolDefinition {
    pub fn validate(&self) -> Result<(), ContractValidationError> {
        self.id.require_kind(CapabilityKind::Tool)?;
        require_text("tool description", &self.description)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderStreamEvent {
    TextDelta {
        text: String,
    },
    ToolCallDelta {
        call_id: String,
        tool_id: CapabilityId,
        arguments_delta: String,
    },
    ToolCall {
        call_id: String,
        tool_id: CapabilityId,
        arguments: Value,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    Finished {
        reason: FinishReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    Cancelled,
}

#[async_trait]
pub trait ProviderCapability: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn authenticate(&self, request: AuthenticationRequest) -> Result<AuthenticationResponse, CapabilityError>;
    async fn stream(
        &self,
        request: ProviderRequest,
    ) -> Result<BoxStream<'static, Result<ProviderStreamEvent, CapabilityError>>, CapabilityError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[default]
    Sequential,
    Parallel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub id: CapabilityId,
    pub description: String,
    pub argument_schema: Value,
    pub prompt_guidance: String,
    pub effects: Vec<OperationEffect>,
    #[serde(default)]
    pub execution_mode: ExecutionMode,
}

impl ToolDescriptor {
    pub fn validate(&self) -> Result<(), ContractValidationError> {
        self.id.require_kind(CapabilityKind::Tool)?;
        require_text("tool description", &self.description)?;
        ensure_normalized_effects(&self.effects)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationEffect {
    ReadPath { scope: PathScope },
    WritePath { scope: PathScope },
    ExecuteProcess,
    Network { access: NetworkAccess },
    UserInteraction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathScope {
    Workspace,
    Explicit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAccess {
    None,
    PublicInternet,
    ExplicitHosts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvocationContext {
    pub session_id: String,
    pub working_directory: String,
    pub has_interactive_ui: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInvocationRequest {
    pub arguments: Value,
    pub context: InvocationContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInvocationResponse {
    pub content: String,
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionOption {
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionRequest {
    pub question: String,
    pub header: Option<String>,
    pub options: Vec<InteractionOption>,
    pub allow_custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionResponse {
    Selected(usize),
    Custom(String),
    Cancelled,
}

#[async_trait]
pub trait ToolHost: Send + Sync {
    async fn interact(&self, request: InteractionRequest) -> Result<InteractionResponse, CapabilityError>;
    fn stream_chunk(&self, _chunk: &str) {}
}

#[async_trait]
pub trait ToolCapability: Send + Sync {
    fn descriptor(&self) -> ToolDescriptor;
    async fn invoke(
        &self,
        host: &dyn ToolHost,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse, CapabilityError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedOperation {
    pub tool_id: CapabilityId,
    pub arguments: Value,
    pub effects: Vec<OperationEffect>,
    pub context: InvocationContext,
}

impl RequestedOperation {
    pub fn normalize(mut self) -> Result<Self, ContractValidationError> {
        self.tool_id.require_kind(CapabilityKind::Tool)?;
        self.effects.sort();
        self.effects.dedup();
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), ContractValidationError> {
        self.tool_id.require_kind(CapabilityKind::Tool)?;
        ensure_normalized_effects(&self.effects)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    ApprovalRequired { rationale: String },
    Deny { rationale: String },
}

impl PermissionDecision {
    pub fn validate(&self) -> Result<(), ContractValidationError> {
        match self {
            Self::Allow => Ok(()),
            Self::ApprovalRequired { rationale } | Self::Deny { rationale } => {
                require_text("permission rationale", rationale)
            }
        }
    }
}

#[async_trait]
pub trait PermissionCapability: Send + Sync {
    fn id(&self) -> CapabilityId;
    async fn evaluate(&self, request: RequestedOperation) -> Result<PermissionDecision, CapabilityError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandDescriptor {
    pub id: CapabilityId,
    pub name: String,
    pub description: String,
}

impl CommandDescriptor {
    pub fn validate(&self) -> Result<(), ContractValidationError> {
        self.id.require_kind(CapabilityKind::Command)?;
        require_text("command name", &self.name)?;
        require_text("command description", &self.description)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandInvocationRequest {
    pub arguments: Vec<String>,
    pub context: InvocationContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandInvocationResponse {
    pub output: String,
    pub exit_code: i32,
}

#[async_trait]
pub trait CommandCapability: Send + Sync {
    fn descriptor(&self) -> CommandDescriptor;
    async fn invoke(&self, request: CommandInvocationRequest) -> Result<CommandInvocationResponse, CapabilityError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LifecycleEvent {
    HostStarted,
    SessionStarted {
        context: InvocationContext,
    },
    BeforeTurn {
        session_id: String,
        prompt: String,
        working_directory: String,
    },
    AfterTurn {
        session_id: String,
        success: bool,
    },
    SessionEnded {
        session_id: String,
    },
    HostStopping,
}

#[async_trait]
pub trait LifecycleCapability: Send + Sync {
    fn id(&self) -> CapabilityId;
    async fn notify(&self, event: LifecycleEvent) -> Result<(), CapabilityError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextDescriptor {
    pub id: CapabilityId,
    pub display_name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_snippets: Option<usize>,
}

impl ContextDescriptor {
    pub fn validate(&self) -> Result<(), ContractValidationError> {
        self.id.require_kind(CapabilityKind::Context)?;
        require_text("context display name", &self.display_name)?;
        require_text("context description", &self.description)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextRequest {
    pub prompt: String,
    pub context: InvocationContext,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSnippet {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

impl PartialEq for ContextSnippet {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
            && self.title == other.title
            && self.content == other.content
            && match (self.score, other.score) {
                (Some(a), Some(b)) => a.to_bits() == b.to_bits(),
                (None, None) => true,
                _ => false,
            }
    }
}

impl Eq for ContextSnippet {}

impl ContextSnippet {
    pub fn validate(&self) -> Result<(), ContractValidationError> {
        require_text("snippet source", &self.source)?;
        require_text("snippet content", &self.content)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextResponse {
    pub snippets: Vec<ContextSnippet>,
}

#[async_trait]
pub trait ContextCapability: Send + Sync {
    fn descriptor(&self) -> ContextDescriptor;
    async fn retrieve(&self, request: ContextRequest) -> Result<ContextResponse, CapabilityError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillAsset {
    pub id: CapabilityId,
    pub name: String,
    pub description: String,
    pub markdown: String,
}

impl SkillAsset {
    pub fn validate(&self) -> Result<(), ContractValidationError> {
        self.id.require_kind(CapabilityKind::Skill)?;
        require_text("skill name", &self.name)?;
        require_text("skill description", &self.description)?;
        require_text("skill markdown", &self.markdown)
    }
}

#[async_trait]
pub trait SkillCapability: Send + Sync {
    fn id(&self) -> CapabilityId;
    async fn assets(&self) -> Result<Vec<SkillAsset>, CapabilityError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "descriptor", rename_all = "snake_case")]
pub enum CapabilityDescriptor {
    Provider(ProviderDescriptor),
    Tool(ToolDescriptor),
    Permission { id: CapabilityId },
    Command(CommandDescriptor),
    Lifecycle { id: CapabilityId },
    Skill { id: CapabilityId },
    Context(ContextDescriptor),
}

impl CapabilityDescriptor {
    pub fn id(&self) -> &CapabilityId {
        match self {
            Self::Provider(descriptor) => &descriptor.id,
            Self::Tool(descriptor) => &descriptor.id,
            Self::Permission { id } | Self::Lifecycle { id } | Self::Skill { id } => id,
            Self::Command(descriptor) => &descriptor.id,
            Self::Context(descriptor) => &descriptor.id,
        }
    }

    pub fn validate(&self) -> Result<(), ContractValidationError> {
        match self {
            Self::Provider(descriptor) => descriptor.validate(),
            Self::Tool(descriptor) => {
                descriptor.validate()?;
                crate::schema::CompiledSchema::compile(&descriptor.argument_schema)
                    .map(|_| ())
                    .map_err(|_| ContractValidationError::InvalidToolSchema)
            }
            Self::Permission { id } => id.require_kind(CapabilityKind::Permission).map_err(Into::into),
            Self::Command(descriptor) => descriptor.validate(),
            Self::Lifecycle { id } => id.require_kind(CapabilityKind::Lifecycle).map_err(Into::into),
            Self::Skill { id } => id.require_kind(CapabilityKind::Skill).map_err(Into::into),
            Self::Context(descriptor) => descriptor.validate(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractValidationError {
    #[error(transparent)]
    Capability(#[from] CapabilityValidationError),
    #[error("{0} must not be empty")]
    EmptyField(&'static str),
    #[error("duplicate provider model: {0}")]
    DuplicateModel(String),
    #[error("operation effects must be sorted and unique")]
    EffectsNotNormalized,
    #[error("tool argument schema is invalid or unsupported")]
    InvalidToolSchema,
}

fn require_text(field: &'static str, value: &str) -> Result<(), ContractValidationError> {
    if value.trim().is_empty() {
        Err(ContractValidationError::EmptyField(field))
    } else {
        Ok(())
    }
}

fn ensure_normalized_effects(effects: &[OperationEffect]) -> Result<(), ContractValidationError> {
    if effects.windows(2).all(|pair| pair[0] < pair[1]) {
        Ok(())
    } else {
        Err(ContractValidationError::EffectsNotNormalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    fn id(value: &str) -> CapabilityId {
        value.parse().unwrap()
    }

    struct Fixture;

    #[async_trait]
    impl ProviderCapability for Fixture {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: id("provider:fixture"),
                display_name: "Fixture".to_string(),
                models: Vec::new(),
                authentication: vec![AuthenticationMethod::None],
            }
        }

        async fn authenticate(
            &self,
            _request: AuthenticationRequest,
        ) -> Result<AuthenticationResponse, CapabilityError> {
            Ok(AuthenticationResponse {
                authenticated: true,
                refreshed_credential: None,
                user_message: None,
            })
        }

        async fn stream(
            &self,
            _request: ProviderRequest,
        ) -> Result<BoxStream<'static, Result<ProviderStreamEvent, CapabilityError>>, CapabilityError> {
            Ok(Box::pin(stream::iter([Ok(ProviderStreamEvent::Finished {
                reason: FinishReason::Stop,
            })])))
        }
    }

    #[async_trait]
    impl ToolCapability for Fixture {
        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                id: id("tool:fixture"),
                description: "Fixture".to_string(),
                argument_schema: serde_json::json!({"type": "object"}),
                prompt_guidance: String::new(),
                effects: Vec::new(),
                execution_mode: ExecutionMode::Parallel,
            }
        }

        async fn invoke(
            &self,
            _host: &dyn ToolHost,
            _request: ToolInvocationRequest,
        ) -> Result<ToolInvocationResponse, CapabilityError> {
            Ok(ToolInvocationResponse {
                content: "ok".to_string(),
                is_error: false,
                structured_content: None,
            })
        }
    }

    #[async_trait]
    impl PermissionCapability for Fixture {
        fn id(&self) -> CapabilityId {
            id("permission:fixture")
        }

        async fn evaluate(&self, _request: RequestedOperation) -> Result<PermissionDecision, CapabilityError> {
            Ok(PermissionDecision::Allow)
        }
    }

    #[async_trait]
    impl CommandCapability for Fixture {
        fn descriptor(&self) -> CommandDescriptor {
            CommandDescriptor {
                id: id("command:fixture"),
                name: "fixture".to_string(),
                description: String::new(),
            }
        }

        async fn invoke(
            &self,
            _request: CommandInvocationRequest,
        ) -> Result<CommandInvocationResponse, CapabilityError> {
            Ok(CommandInvocationResponse {
                output: String::new(),
                exit_code: 0,
            })
        }
    }

    #[async_trait]
    impl LifecycleCapability for Fixture {
        fn id(&self) -> CapabilityId {
            id("lifecycle:fixture")
        }

        async fn notify(&self, _event: LifecycleEvent) -> Result<(), CapabilityError> {
            Ok(())
        }
    }

    #[async_trait]
    impl SkillCapability for Fixture {
        fn id(&self) -> CapabilityId {
            id("skill:fixture")
        }

        async fn assets(&self) -> Result<Vec<SkillAsset>, CapabilityError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl ContextCapability for Fixture {
        fn descriptor(&self) -> ContextDescriptor {
            ContextDescriptor {
                id: id("context:fixture"),
                display_name: "Fixture context".to_string(),
                description: "Fixture context retrieval".to_string(),
                max_snippets: Some(5),
            }
        }

        async fn retrieve(&self, _request: ContextRequest) -> Result<ContextResponse, CapabilityError> {
            Ok(ContextResponse {
                snippets: vec![ContextSnippet {
                    source: "fixture.md".to_string(),
                    title: Some("Fixture Title".to_string()),
                    content: "Fixture content".to_string(),
                    score: Some(0.95),
                }],
            })
        }
    }

    fn assert_contracts<T>()
    where
        T: ProviderCapability
            + ToolCapability
            + PermissionCapability
            + CommandCapability
            + LifecycleCapability
            + SkillCapability
            + ContextCapability,
    {
    }

    #[test]
    fn fixture_implements_every_framework_neutral_contract() {
        assert_contracts::<Fixture>();
    }

    #[test]
    fn validates_contract_kinds_decisions_and_normalized_operations() {
        let descriptor = ToolDescriptor {
            id: id("provider:wrong"),
            description: "tool".to_string(),
            argument_schema: serde_json::json!({}),
            prompt_guidance: String::new(),
            effects: Vec::new(),
            execution_mode: ExecutionMode::Sequential,
        };
        assert!(matches!(
            descriptor.validate(),
            Err(ContractValidationError::Capability(
                CapabilityValidationError::UnexpectedCapabilityKind { .. }
            ))
        ));
        assert_eq!(
            PermissionDecision::Deny {
                rationale: " ".to_string()
            }
            .validate(),
            Err(ContractValidationError::EmptyField("permission rationale"))
        );

        let operation = RequestedOperation {
            tool_id: id("tool:fixture"),
            arguments: serde_json::json!({}),
            effects: vec![OperationEffect::UserInteraction, OperationEffect::ExecuteProcess],
            context: InvocationContext {
                session_id: "session".to_string(),
                working_directory: "/workspace".to_string(),
                has_interactive_ui: true,
            },
        };
        assert_eq!(operation.validate(), Err(ContractValidationError::EffectsNotNormalized));
        operation.normalize().unwrap().validate().unwrap();
    }

    #[test]
    fn credential_debug_output_is_redacted() {
        let secret = "credential-value";
        let credential = ScopedCredential {
            kind: "oauth".to_string(),
            value: serde_json::json!({"access_token": secret}),
        };
        let request = ProviderRequest {
            model: "fixture".to_string(),
            messages: Vec::new(),
            credential: Some(credential),
            max_output_tokens: None,
            tools: Vec::new(),
        };
        let output = format!("{request:?}");
        assert!(!output.contains(secret));
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn contract_types_have_stable_tagged_serialization() {
        assert_eq!(
            serde_json::to_value(ExecutionMode::Parallel).unwrap(),
            serde_json::json!("parallel")
        );
        assert_eq!(
            serde_json::to_value(ExecutionMode::Sequential).unwrap(),
            serde_json::json!("sequential")
        );
        assert_eq!(
            serde_json::from_str::<ExecutionMode>("\"parallel\"").unwrap(),
            ExecutionMode::Parallel
        );
        assert_eq!(
            serde_json::from_str::<ExecutionMode>("\"sequential\"").unwrap(),
            ExecutionMode::Sequential
        );
        assert_eq!(ExecutionMode::default(), ExecutionMode::Sequential);

        let decision = PermissionDecision::ApprovalRequired {
            rationale: "confirm".to_string(),
        };
        assert_eq!(
            serde_json::to_value(decision).unwrap(),
            serde_json::json!({"decision": "approval_required", "rationale": "confirm"})
        );
        let event = ProviderStreamEvent::TextDelta {
            text: "hello".to_string(),
        };
        let encoded = serde_json::to_string(&event).unwrap();
        assert_eq!(serde_json::from_str::<ProviderStreamEvent>(&encoded).unwrap(), event);

        let context_desc = ContextDescriptor {
            id: id("context:fixture"),
            display_name: "Fixture Context".to_string(),
            description: "Retrieval fixture".to_string(),
            max_snippets: Some(10),
        };
        context_desc.validate().unwrap();

        let lifecycle_before = LifecycleEvent::BeforeTurn {
            session_id: "s1".to_string(),
            prompt: "what is rho".to_string(),
            working_directory: "/app".to_string(),
        };
        let encoded = serde_json::to_string(&lifecycle_before).unwrap();
        assert_eq!(
            serde_json::from_str::<LifecycleEvent>(&encoded).unwrap(),
            lifecycle_before
        );

        let lifecycle_after = LifecycleEvent::AfterTurn {
            session_id: "s1".to_string(),
            success: true,
        };
        let encoded = serde_json::to_string(&lifecycle_after).unwrap();
        assert_eq!(
            serde_json::from_str::<LifecycleEvent>(&encoded).unwrap(),
            lifecycle_after
        );
    }
}
