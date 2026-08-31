use crate::auth::CredentialScope;
use crate::catalog::{ModelCatalog, curated};
use async_trait::async_trait;
use futures::stream::BoxStream;
use rho_core::error::{AppError, Result};
use rho_core::provider::{CredentialStrategy, ProviderId};
use rho_host::external::ExternalProvider;
use rho_sdk::capability::{CapabilityError, CapabilityId, PluginId};
use rho_sdk::contract::{
    AuthenticationMethod, AuthenticationRequest, AuthenticationResponse, ModelMetadata, ProviderCapability,
    ProviderDescriptor, ProviderRequest, ProviderStreamEvent,
};
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
    pub(crate) id: ProviderId,
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
                ProviderId::ChatGpt | ProviderId::Antigravity | ProviderId::XAi | ProviderId::Cohere => {
                    CatalogSource::Curated
                }
                _ => CatalogSource::Live,
            },
            supports_quota: self.id == ProviderId::ChatGpt || self.id == ProviderId::Antigravity,
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
                supports_images: matches!(
                    self.id,
                    ProviderId::Anthropic | ProviderId::OpenAi | ProviderId::Gemini | ProviderId::Antigravity
                ),
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
