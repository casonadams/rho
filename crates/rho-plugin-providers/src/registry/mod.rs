pub mod types;

use crate::auth::AuthStore;
use rho_core::config::Config;
use rho_core::error::{AppError, Result};
use rho_core::provider::ProviderId;
use rho_host::external::ExternalPlugin;
use rho_host::loader::{ConfiguredStatus, PluginLoader};
use rho_host::process::ProcessLimits;
use rho_sdk::contract::ProviderDescriptor;
use std::collections::BTreeMap;

pub use types::{ActiveProvider, BuiltinProvider, CatalogSource, ProviderFacts, capability_id, context_limit};

pub struct ProviderRegistry {
    providers: BTreeMap<String, ActiveProvider>,
}

impl ProviderRegistry {
    pub async fn load(config: &Config) -> Result<Self> {
        let mut providers = ProviderId::ALL
            .into_iter()
            .map(|id| {
                (
                    id.as_str().to_string(),
                    ActiveProvider::Builtin(BuiltinProvider::new(id)),
                )
            })
            .collect::<BTreeMap<_, _>>();

        for candidate in PluginLoader::configured_candidates(&config.config_dir, &config.plugins) {
            if candidate.status != ConfiguredStatus::Eligible {
                continue;
            }
            let plugin = match ExternalPlugin::load(&candidate.path, ProcessLimits::default()).await {
                Ok(plugin) => plugin,
                Err(_) => continue,
            };
            if plugin.manifest().plugin_id.as_str() != candidate.name {
                continue;
            }
            let plugin_id = plugin.manifest().plugin_id.clone();
            for declaration in &plugin.manifest().capabilities {
                if declaration.id.kind() != rho_sdk::capability::CapabilityKind::Provider {
                    continue;
                }
                let target = declaration.replaces.as_ref().unwrap_or(&declaration.id);
                if target != &declaration.id && !candidate.replaces.contains(target) {
                    continue;
                }
                let Ok(provider) = plugin.provider(&declaration.id) else {
                    continue;
                };
                providers.insert(
                    target.name().to_string(),
                    ActiveProvider::External {
                        plugin_id: plugin_id.clone(),
                        capability: Box::new(provider),
                    },
                );
            }
        }
        Ok(Self { providers })
    }

    pub fn builtins() -> Self {
        Self {
            providers: ProviderId::ALL
                .into_iter()
                .map(|id| {
                    (
                        id.as_str().to_string(),
                        ActiveProvider::Builtin(BuiltinProvider::new(id)),
                    )
                })
                .collect(),
        }
    }

    pub fn get(&self, name: &str) -> Result<&ActiveProvider> {
        let normalized = name.trim().to_ascii_lowercase();
        let normalized = normalized
            .parse::<ProviderId>()
            .map_or(normalized, |provider| provider.as_str().to_string());
        self.providers
            .get(&normalized)
            .ok_or_else(|| AppError::Provider(format!("Unknown or unsupported AI provider: '{name}'")))
    }

    pub fn descriptors(&self) -> Vec<ProviderDescriptor> {
        self.providers.values().map(ActiveProvider::descriptor).collect()
    }

    pub fn selected_credential(
        &self,
        name: &str,
        store: &AuthStore,
    ) -> Result<Option<(u64, rho_sdk::contract::ScopedCredential)>> {
        Ok(store.scoped_credential(&self.get(name)?.credential_scope()))
    }
}

#[cfg(test)]
mod tests;
