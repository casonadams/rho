use crate::plugin::capability::{CapabilityError, CapabilityId, CapabilityKind, ValidatedManifest};
use crate::plugin::contract::{
    AuthenticationRequest, AuthenticationResponse, CapabilityDescriptor, CommandCapability, CommandDescriptor,
    CommandInvocationRequest, CommandInvocationResponse, LifecycleCapability, LifecycleEvent, PermissionCapability,
    PermissionDecision, ProviderCapability, ProviderDescriptor, ProviderRequest, ProviderStreamEvent,
    RequestedOperation, SkillAsset, SkillCapability, ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest,
    ToolInvocationResponse,
};
use crate::plugin::process::{PluginProcessClient, ProcessError, ProcessLimits};
use crate::plugin::protocol::{InvocationRequest, StreamEvent, TerminalResult};
use async_trait::async_trait;
use futures::stream::BoxStream;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct ExternalPlugin {
    client: PluginProcessClient,
    manifest: ValidatedManifest,
    descriptors: Arc<BTreeMap<CapabilityId, CapabilityDescriptor>>,
    unavailable: Arc<BTreeMap<CapabilityId, String>>,
}

impl ExternalPlugin {
    pub async fn load(executable: impl Into<PathBuf>, limits: ProcessLimits) -> Result<Self, CapabilityError> {
        let client = PluginProcessClient::new(executable, limits);
        let discovery = client.discover().await.map_err(capability_error)?;
        let declared: BTreeSet<_> = discovery
            .manifest
            .capabilities
            .iter()
            .map(|declaration| declaration.id.clone())
            .collect();
        let mut descriptors = BTreeMap::new();
        let mut unavailable = BTreeMap::new();
        for descriptor in discovery.capabilities {
            let id = descriptor.id().clone();
            if !declared.contains(&id) {
                continue;
            }
            if descriptor.validate().is_err() {
                unavailable.insert(id, "capability declaration failed validation".to_string());
            } else if descriptors.insert(id.clone(), descriptor).is_some() {
                descriptors.remove(&id);
                unavailable.insert(id, "capability was declared more than once".to_string());
            }
        }
        for id in declared {
            if !descriptors.contains_key(&id) && !unavailable.contains_key(&id) {
                unavailable.insert(id, "capability descriptor is missing".to_string());
            }
        }
        Ok(Self {
            client,
            manifest: discovery.manifest,
            descriptors: Arc::new(descriptors),
            unavailable: Arc::new(unavailable),
        })
    }

    pub fn manifest(&self) -> &ValidatedManifest {
        &self.manifest
    }

    pub fn unavailable(&self) -> &BTreeMap<CapabilityId, String> {
        &self.unavailable
    }

    pub fn resolvable_manifest(&self) -> ValidatedManifest {
        ValidatedManifest {
            plugin_id: self.manifest.plugin_id.clone(),
            plugin_version: self.manifest.plugin_version.clone(),
            api_version: self.manifest.api_version,
            protocol_version: self.manifest.protocol_version,
            capabilities: self
                .manifest
                .capabilities
                .iter()
                .filter(|declaration| self.descriptors.contains_key(&declaration.id))
                .cloned()
                .collect(),
        }
    }

    pub fn provider(&self, id: &CapabilityId) -> Result<ExternalProvider, CapabilityError> {
        match self.descriptor(id)? {
            CapabilityDescriptor::Provider(descriptor) => Ok(ExternalProvider {
                client: self.client.clone(),
                descriptor: descriptor.clone(),
            }),
            _ => Err(wrong_kind(id)),
        }
    }

    pub fn tool(&self, id: &CapabilityId) -> Result<ExternalTool, CapabilityError> {
        match self.descriptor(id)? {
            CapabilityDescriptor::Tool(descriptor) => {
                let schema = crate::plugin::schema::CompiledSchema::compile(&descriptor.argument_schema)
                    .map_err(|_| invalid_capability(id))?;
                Ok(ExternalTool {
                    client: self.client.clone(),
                    descriptor: descriptor.clone(),
                    schema: Arc::new(schema),
                })
            }
            _ => Err(wrong_kind(id)),
        }
    }

    pub fn permission(&self, id: &CapabilityId) -> Result<ExternalPermission, CapabilityError> {
        match self.descriptor(id)? {
            CapabilityDescriptor::Permission { .. } => Ok(ExternalPermission {
                client: self.client.clone(),
                id: id.clone(),
            }),
            _ => Err(wrong_kind(id)),
        }
    }

    pub fn command(&self, id: &CapabilityId) -> Result<ExternalCommand, CapabilityError> {
        match self.descriptor(id)? {
            CapabilityDescriptor::Command(descriptor) => Ok(ExternalCommand {
                client: self.client.clone(),
                descriptor: descriptor.clone(),
            }),
            _ => Err(wrong_kind(id)),
        }
    }

    pub fn lifecycle(&self, id: &CapabilityId) -> Result<ExternalLifecycle, CapabilityError> {
        match self.descriptor(id)? {
            CapabilityDescriptor::Lifecycle { .. } => Ok(ExternalLifecycle {
                client: self.client.clone(),
                id: id.clone(),
            }),
            _ => Err(wrong_kind(id)),
        }
    }

    pub fn skill(&self, id: &CapabilityId) -> Result<ExternalSkill, CapabilityError> {
        match self.descriptor(id)? {
            CapabilityDescriptor::Skill { .. } => Ok(ExternalSkill {
                client: self.client.clone(),
                id: id.clone(),
            }),
            _ => Err(wrong_kind(id)),
        }
    }

    fn descriptor(&self, id: &CapabilityId) -> Result<&CapabilityDescriptor, CapabilityError> {
        if let Some(reason) = self.unavailable.get(id) {
            return Err(CapabilityError::Unavailable {
                message: format!("{id}: {reason}"),
            });
        }
        self.descriptors.get(id).ok_or_else(|| CapabilityError::Unavailable {
            message: format!("{id} was not declared by the plugin"),
        })
    }
}

#[derive(Clone)]
pub struct ExternalProvider {
    client: PluginProcessClient,
    descriptor: ProviderDescriptor,
}

#[async_trait]
impl ProviderCapability for ExternalProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }

    async fn authenticate(&self, request: AuthenticationRequest) -> Result<AuthenticationResponse, CapabilityError> {
        let output = self
            .client
            .invoke(
                self.descriptor.id.clone(),
                InvocationRequest::ProviderAuthenticate(request),
            )
            .await
            .map_err(capability_error)?;
        if !output.events.is_empty() {
            return Err(invalid_response());
        }
        match output.terminal {
            TerminalResult::ProviderAuthenticated(response) => Ok(response),
            _ => Err(invalid_response()),
        }
    }

    async fn stream(
        &self,
        request: ProviderRequest,
    ) -> Result<BoxStream<'static, Result<ProviderStreamEvent, CapabilityError>>, CapabilityError> {
        let running = self
            .client
            .start_invocation(self.descriptor.id.clone(), InvocationRequest::ProviderStream(request))
            .await
            .map_err(capability_error)?;
        let (sender, receiver) = mpsc::channel(32);
        tokio::spawn(async move {
            let mut running = running;
            loop {
                let event = tokio::select! {
                    _ = sender.closed() => {
                        let _ = running.cancel().await;
                        return;
                    }
                    event = running.next_event() => event,
                };
                let Some(event) = event else {
                    break;
                };
                let StreamEvent::Provider(event) = event else {
                    let _ = sender.send(Err(invalid_response())).await;
                    let _ = running.cancel().await;
                    return;
                };
                if validate_provider_event(&event).is_err() {
                    let _ = sender.send(Err(invalid_response())).await;
                    let _ = running.cancel().await;
                    return;
                }
                if sender.send(Ok(event)).await.is_err() {
                    let _ = running.cancel().await;
                    return;
                }
            }
            match running.finish().await {
                Ok(TerminalResult::StreamCompleted) => {}
                Ok(_) => {
                    let _ = sender.send(Err(invalid_response())).await;
                }
                Err(error) => {
                    let _ = sender.send(Err(capability_error(error))).await;
                }
            }
        });
        Ok(Box::pin(futures::stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|event| (event, receiver))
        })))
    }
}

#[derive(Clone)]
pub struct ExternalTool {
    client: PluginProcessClient,
    descriptor: ToolDescriptor,
    schema: Arc<crate::plugin::schema::CompiledSchema>,
}

#[async_trait]
impl ToolCapability for ExternalTool {
    fn descriptor(&self) -> ToolDescriptor {
        self.descriptor.clone()
    }

    async fn invoke(
        &self,
        _host: &dyn ToolHost,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse, CapabilityError> {
        self.schema
            .validate(&request.arguments)
            .map_err(|_| CapabilityError::InvalidRequest {
                message: "tool arguments do not match the declared schema".to_string(),
            })?;
        match invoke_terminal(
            &self.client,
            self.descriptor.id.clone(),
            InvocationRequest::Tool(request),
        )
        .await?
        {
            TerminalResult::Tool(response) => Ok(response),
            _ => Err(invalid_response()),
        }
    }
}

#[derive(Clone)]
pub struct ExternalPermission {
    client: PluginProcessClient,
    id: CapabilityId,
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

#[derive(Clone)]
pub struct ExternalCommand {
    client: PluginProcessClient,
    descriptor: CommandDescriptor,
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
    client: PluginProcessClient,
    id: CapabilityId,
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
    client: PluginProcessClient,
    id: CapabilityId,
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

async fn invoke_terminal(
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

fn validate_provider_event(event: &ProviderStreamEvent) -> Result<(), ()> {
    match event {
        ProviderStreamEvent::ToolCallDelta { tool_id, .. } | ProviderStreamEvent::ToolCall { tool_id, .. } => {
            tool_id.require_kind(CapabilityKind::Tool).map_err(|_| ())
        }
        _ => Ok(()),
    }
}

fn capability_error(error: ProcessError) -> CapabilityError {
    match error {
        ProcessError::Remote {
            code: crate::plugin::protocol::ErrorCode::Cancelled,
            ..
        } => CapabilityError::Cancelled,
        other => CapabilityError::Unavailable {
            message: other.to_string(),
        },
    }
}

fn invalid_capability(id: &CapabilityId) -> CapabilityError {
    CapabilityError::Unavailable {
        message: format!("{id} failed capability validation"),
    }
}

fn wrong_kind(id: &CapabilityId) -> CapabilityError {
    CapabilityError::Unavailable {
        message: format!("{id} has an unexpected capability kind"),
    }
}

fn invalid_response() -> CapabilityError {
    CapabilityError::Failed {
        message: "plugin returned an invalid capability response".to_string(),
    }
}

#[cfg(all(test, unix))]
mod tests;
