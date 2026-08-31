//! Credential verification: confirm a freshly captured API key against the
//! upstream provider before persisting it.
//!
//! Providers without an API key (OAuth subscription providers and Ollama)
//! return [`VerificationStatus::Deferred`] — the actual verification happens
//! later when the token is exchanged.

use crate::auth::{ApiKeyVerifier, VerificationStatus};
use rho_core::error::{AppError, Result};
use rho_core::provider::ProviderId;
use rig::client::VerifyClient;

pub struct RigCredentialVerifier;

#[async_trait::async_trait]
impl ApiKeyVerifier for RigCredentialVerifier {
    async fn verify(&self, provider: ProviderId, key: &str) -> Result<VerificationStatus> {
        match provider {
            ProviderId::Anthropic => verify(rig::providers::anthropic::Client::new(key)).await,
            ProviderId::OpenAi => verify(rig::providers::openai::Client::new(key)).await,
            ProviderId::DeepSeek => verify(rig::providers::deepseek::Client::new(key)).await,
            ProviderId::Gemini => verify(rig::providers::gemini::Client::new(key)).await,
            ProviderId::Groq => verify(rig::providers::groq::Client::new(key)).await,
            ProviderId::OpenRouter => verify(rig::providers::openrouter::Client::new(key)).await,
            ProviderId::XAi => verify(rig::providers::xai::Client::new(key)).await,
            ProviderId::Mistral => verify(rig::providers::mistral::Client::new(key)).await,
            ProviderId::Cohere => verify(rig::providers::cohere::Client::new(key)).await,
            ProviderId::ChatGpt | ProviderId::Copilot | ProviderId::Antigravity | ProviderId::Ollama => {
                Ok(VerificationStatus::Deferred)
            }
        }
    }
}

async fn verify<C, E>(client: std::result::Result<C, E>) -> Result<VerificationStatus>
where
    C: VerifyClient,
{
    let client = client.map_err(|_| AppError::Auth("Failed to initialize credential verification".to_string()))?;
    client
        .verify()
        .await
        .map_err(|_| AppError::Auth("Credential verification failed; stored key was not changed".to_string()))?;
    Ok(VerificationStatus::Verified)
}
