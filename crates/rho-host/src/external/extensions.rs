use super::common::{invalid_response, invoke_terminal};
use crate::process::PluginProcessClient;
use async_trait::async_trait;
use rho_sdk::capability::{CapabilityError, CapabilityId};
use rho_sdk::contract::{
    CommandCapability, CommandDescriptor, CommandInvocationRequest, CommandInvocationResponse, ContextCapability,
    ContextDescriptor, ContextRequest, ContextResponse, LifecycleCapability, LifecycleEvent, SkillAsset,
    SkillCapability,
};
use rho_sdk::protocol::{InvocationRequest, TerminalResult};

#[derive(Clone)]
pub struct ExternalCommand {
    pub(crate) client: PluginProcessClient,
    pub(crate) descriptor: CommandDescriptor,
}

#[async_trait]
impl CommandCapability for ExternalCommand {
    fn descriptor(&self) -> CommandDescriptor {
        self.descriptor.clone()
    }

    async fn invoke(&self, request: CommandInvocationRequest) -> Result<CommandInvocationResponse, CapabilityError> {
        match invoke_terminal(
            &self.client,
            self.descriptor.id.clone(),
            InvocationRequest::Command(request),
        )
        .await?
        {
            TerminalResult::Command(response) => Ok(response),
            _ => Err(invalid_response()),
        }
    }
}

#[derive(Clone)]
pub struct ExternalLifecycle {
    pub(crate) client: PluginProcessClient,
    pub(crate) id: CapabilityId,
}

#[async_trait]
impl LifecycleCapability for ExternalLifecycle {
    fn id(&self) -> CapabilityId {
        self.id.clone()
    }

    async fn notify(&self, event: LifecycleEvent) -> Result<(), CapabilityError> {
        match invoke_terminal(&self.client, self.id.clone(), InvocationRequest::Lifecycle(event)).await? {
            TerminalResult::Lifecycle => Ok(()),
            _ => Err(invalid_response()),
        }
    }
}

#[derive(Clone)]
pub struct ExternalSkill {
    pub(crate) client: PluginProcessClient,
    pub(crate) id: CapabilityId,
}

#[async_trait]
impl SkillCapability for ExternalSkill {
    fn id(&self) -> CapabilityId {
        self.id.clone()
    }

    async fn assets(&self) -> Result<Vec<SkillAsset>, CapabilityError> {
        match invoke_terminal(&self.client, self.id.clone(), InvocationRequest::Skills).await? {
            TerminalResult::Skills(assets) => {
                for asset in &assets {
                    asset.validate().map_err(|_| invalid_response())?;
                }
                Ok(assets)
            }
            _ => Err(invalid_response()),
        }
    }
}

#[derive(Clone)]
pub struct ExternalContext {
    pub(crate) client: PluginProcessClient,
    pub(crate) descriptor: ContextDescriptor,
}

#[async_trait]
impl ContextCapability for ExternalContext {
    fn descriptor(&self) -> ContextDescriptor {
        self.descriptor.clone()
    }

    async fn retrieve(&self, request: ContextRequest) -> Result<ContextResponse, CapabilityError> {
        match invoke_terminal(
            &self.client,
            self.descriptor.id.clone(),
            InvocationRequest::Context(request),
        )
        .await?
        {
            TerminalResult::Context(response) => {
                for snippet in &response.snippets {
                    snippet.validate().map_err(|_| invalid_response())?;
                }
                Ok(response)
            }
            _ => Err(invalid_response()),
        }
    }
}
