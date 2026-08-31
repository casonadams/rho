use crate::process::{PluginProcessClient, ProcessError};
use rho_sdk::capability::{CapabilityError, CapabilityId, CapabilityKind};
use rho_sdk::contract::ProviderStreamEvent;
use rho_sdk::protocol::{InvocationRequest, TerminalResult};

pub(crate) async fn invoke_terminal(
    client: &PluginProcessClient,
    id: CapabilityId,
    request: InvocationRequest,
) -> Result<TerminalResult, CapabilityError> {
    let output = client.invoke(id, request).await.map_err(capability_error)?;
    if output.events.is_empty() {
        Ok(output.terminal)
    } else {
        Err(invalid_response())
    }
}

pub(crate) fn validate_provider_event(event: &ProviderStreamEvent) -> Result<(), ()> {
    match event {
        ProviderStreamEvent::ToolCallDelta { tool_id, .. } | ProviderStreamEvent::ToolCall { tool_id, .. } => {
            tool_id.require_kind(CapabilityKind::Tool).map_err(|_| ())
        }
        _ => Ok(()),
    }
}

pub(crate) fn capability_error(error: ProcessError) -> CapabilityError {
    match error {
        ProcessError::Remote {
            code: rho_sdk::protocol::ErrorCode::Cancelled,
            ..
        } => CapabilityError::Cancelled,
        other => CapabilityError::Unavailable {
            message: other.to_string(),
        },
    }
}

pub(crate) fn invalid_capability(id: &CapabilityId) -> CapabilityError {
    CapabilityError::Unavailable {
        message: format!("{id} failed capability validation"),
    }
}

pub(crate) fn wrong_kind(id: &CapabilityId) -> CapabilityError {
    CapabilityError::Unavailable {
        message: format!("{id} has an unexpected capability kind"),
    }
}

pub(crate) fn invalid_response() -> CapabilityError {
    CapabilityError::Failed {
        message: "plugin returned an invalid capability response".to_string(),
    }
}
