use super::tool::InvocationContext;
use super::validation::{ContractValidationError, require_text};
use crate::capability::{CapabilityError, CapabilityId, CapabilityKind};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

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
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files_modified: Vec<String>,
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
