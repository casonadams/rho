use super::catalog::ModelCatalog;
use super::catalog::curated;
use crate::auth::{AuthStore, CredentialScope};
use async_trait::async_trait;
use futures::stream::BoxStream;
use rho_core::config::Config;
use rho_core::error::{AppError, Result};
use rho_core::provider::CredentialStrategy;
use rho_core::provider::ProviderId;
use rho_host::external::{ExternalPlugin, ExternalProvider};
use rho_host::loader::{ConfiguredStatus, PluginLoader};
use rho_host::process::ProcessLimits;
use rho_sdk::capability::{CapabilityError, CapabilityId, PluginId};
use rho_sdk::contract::{
    AuthenticationMethod, AuthenticationRequest, AuthenticationResponse, ModelMetadata, ProviderCapability,
    ProviderDescriptor, ProviderRequest, ProviderStreamEvent,
};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogSource {
    Live,
    Curated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderFacts {
    pub provider: ProviderId,
    pub catalog_source: CatalogSource,
    pub supports_quota: bool,
    pub supports_status: bool,
}

#[derive(Clone)]
pub enum ActiveProvider {
    Builtin(BuiltinProvider),
    External {
        plugin_id: PluginId,
        capability: Box<ExternalProvider>,
    },
}

impl ActiveProvider {
    pub fn descriptor(&self) -> ProviderDescriptor {
        match self {
            Self::Builtin(provider) => provider.descriptor(),
            Self::External { capability, .. } => capability.descriptor(),
        }
    }

    pub fn credential_strategy(&self) -> CredentialStrategy {
        match self.descriptor().authentication.first() {
            Some(AuthenticationMethod::ApiKey { .. }) => CredentialStrategy::ApiKey,
            Some(AuthenticationMethod::OAuth { .. }) => CredentialStrategy::SubscriptionOAuth,
            _ => CredentialStrategy::Local,
        }
    }

    pub fn capability(&self) -> Option<Arc<dyn ProviderCapability>> {
        match self {
            Self::Builtin(_) => None,
            Self::External { capability, .. } => Some(Arc::new((**capability).clone())),
        }
    }

    pub fn credential_scope(&self) -> CredentialScope {
        match self {
            Self::Builtin(provider) => CredentialScope::builtin_provider(provider.id),
            Self::External { plugin_id, capability } => CredentialScope {
                plugin_id: plugin_id.clone(),
                capability_id: capability.descriptor().id,
                account_id: None,
            },
        }
    }
}

#[derive(Clone, Copy)]
pub struct BuiltinProvider {
    id: ProviderId,
}

impl BuiltinProvider {
    pub fn new(id: ProviderId) -> Self {
        Self { id }
    }

    pub fn id(self) -> ProviderId {
        self.id
    }

    pub fn facts(self) -> ProviderFacts {
        ProviderFacts {
            provider: self.id,
            catalog_source: match self.id {
                ProviderId::ChatGpt | ProviderId::XAi | ProviderId::Cohere => CatalogSource::Curated,
                _ => CatalogSource::Live,
            },
            supports_quota: self.id == ProviderId::ChatGpt,
            supports_status: true,
        }
    }

    pub fn model_catalog(self) -> ModelCatalog {
        curated(self.id)
    }
}

#[async_trait]
impl ProviderCapability for BuiltinProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        let models = curated(self.id)
            .models()
            .into_iter()
            .map(|model| ModelMetadata {
                id: model.to_string(),
                display_name: model.to_string(),
                context_limit: context_limit(model).map(|limit| limit as u64),
                supports_tools: true,
                supports_images: matches!(self.id, ProviderId::Anthropic | ProviderId::OpenAi | ProviderId::Gemini),
            })
            .collect();
        ProviderDescriptor {
            id: format!("provider:{}", self.id.as_str()).parse().unwrap(),
            display_name: self.id.to_string(),
            models,
            authentication: vec![match self.id.credential_strategy() {
                CredentialStrategy::ApiKey => AuthenticationMethod::ApiKey {
                    label: "API key".to_string(),
                },
                CredentialStrategy::SubscriptionOAuth => AuthenticationMethod::OAuth {
                    label: "subscription OAuth".to_string(),
                },
                CredentialStrategy::Local => AuthenticationMethod::None,
            }],
        }
    }

    async fn authenticate(
        &self,
        _request: AuthenticationRequest,
    ) -> std::result::Result<AuthenticationResponse, CapabilityError> {
        Err(CapabilityError::Unavailable {
            message: "built-in authentication is coordinated by the host".to_string(),
        })
    }

    async fn stream(
        &self,
        _request: ProviderRequest,
    ) -> std::result::Result<
        BoxStream<'static, std::result::Result<ProviderStreamEvent, CapabilityError>>,
        CapabilityError,
    > {
        Err(CapabilityError::Unavailable {
            message: "built-in streaming is handled by the internal Rig adapter".to_string(),
        })
    }
}

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

pub fn context_limit(model: &str) -> Option<usize> {
    let lower = model.to_ascii_lowercase();
    if lower.contains("gpt-5") || lower.contains("luna") || lower.contains("codex") {
        Some(376_000)
    } else if lower.contains("o1") || lower.contains("o3") {
        Some(200_000)
    } else if lower.contains("gpt-4") || lower.contains("deepseek") {
        Some(128_000)
    } else if lower.contains("claude") || lower.contains("gemini") {
        Some(1_000_000)
    } else {
        None
    }
}

pub fn capability_id(name: &str) -> Result<CapabilityId> {
    format!("provider:{name}")
        .parse()
        .map_err(|error: rho_sdk::capability::CapabilityValidationError| AppError::Provider(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_parity_matrix_covers_catalog_auth_context_quota_and_status() {
        let expected = [
            (ProviderId::Anthropic, CredentialStrategy::ApiKey, false),
            (ProviderId::OpenAi, CredentialStrategy::ApiKey, false),
            (ProviderId::ChatGpt, CredentialStrategy::SubscriptionOAuth, true),
            (ProviderId::Copilot, CredentialStrategy::SubscriptionOAuth, false),
            (ProviderId::DeepSeek, CredentialStrategy::ApiKey, false),
            (ProviderId::Gemini, CredentialStrategy::ApiKey, false),
            (ProviderId::Groq, CredentialStrategy::ApiKey, false),
            (ProviderId::Ollama, CredentialStrategy::Local, false),
            (ProviderId::OpenRouter, CredentialStrategy::ApiKey, false),
            (ProviderId::XAi, CredentialStrategy::ApiKey, false),
            (ProviderId::Mistral, CredentialStrategy::ApiKey, false),
            (ProviderId::Cohere, CredentialStrategy::ApiKey, false),
        ];
        for (id, strategy, quota) in expected {
            let provider = BuiltinProvider::new(id);
            let descriptor = provider.descriptor();
            assert_eq!(descriptor.id.name(), id.as_str());
            assert_eq!(id.credential_strategy(), strategy);
            assert_eq!(ActiveProvider::Builtin(provider).credential_strategy(), strategy);
            assert_eq!(provider.facts().supports_quota, quota);
            assert!(provider.facts().supports_status);
            assert_eq!(descriptor.models.len(), provider.model_catalog().models().len());
            for model in descriptor.models {
                assert_eq!(model.context_limit, context_limit(&model.id).map(|limit| limit as u64));
            }
        }
    }

    #[test]
    fn local_provider_declares_no_auth_and_unknown_provider_fails() {
        let registry = ProviderRegistry::builtins();
        assert_eq!(
            registry.get("ollama").unwrap().descriptor().authentication,
            vec![AuthenticationMethod::None]
        );
        assert!(registry.get("unknown").is_err());
    }
}
