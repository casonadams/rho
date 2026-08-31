use super::common::{capability_error, invalid_response};
use crate::process::PluginProcessClient;
use async_trait::async_trait;
use rho_sdk::capability::CapabilityError;
use rho_sdk::contract::{ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest, ToolInvocationResponse};
use rho_sdk::protocol::{InvocationRequest, StreamEvent, TerminalResult};
use std::sync::Arc;

#[derive(Clone)]
pub struct ExternalTool {
    pub(crate) client: PluginProcessClient,
    pub(crate) descriptor: ToolDescriptor,
    pub(crate) schema: Arc<rho_sdk::schema::CompiledSchema>,
}

#[async_trait]
impl ToolCapability for ExternalTool {
    fn descriptor(&self) -> ToolDescriptor {
        self.descriptor.clone()
    }

    async fn invoke(
        &self,
        host: &dyn ToolHost,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse, CapabilityError> {
        self.schema
            .validate(&request.arguments)
            .map_err(|_| CapabilityError::InvalidRequest {
                message: "tool arguments do not match the declared schema".to_string(),
            })?;
        let output = self
            .client
            .invoke(self.descriptor.id.clone(), InvocationRequest::Tool(request))
            .await
            .map_err(capability_error)?;
        for event in output.events {
            match event {
                StreamEvent::Progress { message } => host.stream_chunk(&message),
                StreamEvent::CommandOutput { content } => host.stream_chunk(&content),
                _ => {}
            }
        }
        match output.terminal {
            TerminalResult::Tool(response) => Ok(response),
            _ => Err(invalid_response()),
        }
    }
}
