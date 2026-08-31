pub mod common;
pub mod extensions;
pub mod permission;
pub mod provider;
#[cfg(all(test, unix))]
mod tests;
pub mod tool;

pub use extensions::{ExternalCommand, ExternalContext, ExternalLifecycle, ExternalSkill};
pub use permission::ExternalPermission;
pub use provider::ExternalProvider;
pub use tool::ExternalTool;

use crate::process::{PluginProcessClient, ProcessLimits};
use common::{capability_error, invalid_capability, wrong_kind};
use rho_sdk::capability::{CapabilityError, CapabilityId, ValidatedManifest};
use rho_sdk::contract::CapabilityDescriptor;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct ExternalPlugin {
    client: PluginProcessClient,
    manifest: ValidatedManifest,
    descriptors: Arc<BTreeMap<CapabilityId, CapabilityDescriptor>>,
    unavailable: Arc<BTreeMap<CapabilityId, String>>,
}

impl ExternalPlugin {
    pub async fn load(executable: impl Into<PathBuf>, limits: ProcessLimits) -> Result<Self, CapabilityError> {
        Self::load_with_config(executable, limits, None).await
    }

    pub async fn load_with_config(
        executable: impl Into<PathBuf>,
        limits: ProcessLimits,
        config: Option<serde_json::Value>,
    ) -> Result<Self, CapabilityError> {
        let client = PluginProcessClient::with_config(executable, limits, config);
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
                let schema = rho_sdk::schema::CompiledSchema::compile(&descriptor.argument_schema)
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

    pub fn context(&self, id: &CapabilityId) -> Result<ExternalContext, CapabilityError> {
        match self.descriptor(id)? {
            CapabilityDescriptor::Context(descriptor) => Ok(ExternalContext {
                client: self.client.clone(),
                descriptor: descriptor.clone(),
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
