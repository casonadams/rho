pub mod extensions;
pub mod permission;
pub mod provider;
#[cfg(test)]
mod tests;
pub mod tool;
pub mod validation;

pub use extensions::{
    CommandCapability, CommandDescriptor, CommandInvocationRequest, CommandInvocationResponse, ContextCapability,
    ContextDescriptor, ContextRequest, ContextResponse, ContextSnippet, LifecycleCapability, LifecycleEvent,
    SkillAsset, SkillCapability,
};
pub use permission::{PermissionCapability, PermissionDecision, RequestedOperation};
pub use provider::{
    AuthenticationMethod, AuthenticationOperation, AuthenticationRequest, AuthenticationResponse, FinishReason,
    MessageContent, MessageRole, ModelMessage, ModelMetadata, ProviderCapability, ProviderDescriptor, ProviderRequest,
    ProviderStreamEvent, ProviderToolDefinition, ScopedCredential,
};
pub use tool::{
    ExecutionMode, InteractionOption, InteractionRequest, InteractionResponse, InvocationContext, NetworkAccess,
    OperationEffect, PathScope, ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest,
    ToolInvocationResponse,
};
pub use validation::ContractValidationError;

use crate::capability::{CapabilityId, CapabilityKind};
use serde::{Deserialize, Serialize};

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
