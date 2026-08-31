use super::common::{invalid_response, invoke_terminal};
use crate::process::PluginProcessClient;
use async_trait::async_trait;
use rho_sdk::capability::{CapabilityError, CapabilityId};
use rho_sdk::contract::{PermissionCapability, PermissionDecision, RequestedOperation};
use rho_sdk::protocol::{InvocationRequest, TerminalResult};

#[derive(Clone)]
pub struct ExternalPermission {
    pub(crate) client: PluginProcessClient,
    pub(crate) id: CapabilityId,
}

#[async_trait]
impl PermissionCapability for ExternalPermission {
    fn id(&self) -> CapabilityId {
        self.id.clone()
    }

    async fn evaluate(&self, request: RequestedOperation) -> Result<PermissionDecision, CapabilityError> {
        let request = request.normalize().map_err(|_| CapabilityError::InvalidRequest {
            message: "permission request is not normalized".to_string(),
        })?;
        match invoke_terminal(&self.client, self.id.clone(), InvocationRequest::Permission(request)).await? {
            TerminalResult::Permission(decision) => {
                decision.validate().map_err(|_| invalid_response())?;
                Ok(decision)
            }
            _ => Err(invalid_response()),
        }
    }
}
