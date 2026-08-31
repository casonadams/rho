use super::tool::OperationEffect;
use crate::capability::CapabilityValidationError;

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

pub(crate) fn require_text(field: &'static str, value: &str) -> Result<(), ContractValidationError> {
    if value.trim().is_empty() {
        Err(ContractValidationError::EmptyField(field))
    } else {
        Ok(())
    }
}

pub(crate) fn ensure_normalized_effects(effects: &[OperationEffect]) -> Result<(), ContractValidationError> {
    if effects.windows(2).all(|pair| pair[0] < pair[1]) {
        Ok(())
    } else {
        Err(ContractValidationError::EffectsNotNormalized)
    }
}
