use super::tool::{InvocationContext, OperationEffect};
use super::validation::{ContractValidationError, ensure_normalized_effects, require_text};
use crate::capability::{CapabilityError, CapabilityId, CapabilityKind};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
