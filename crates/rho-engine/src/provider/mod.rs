pub mod adapter;
mod builders;
pub mod catalog;
pub mod discovery;
pub mod error;
pub mod openai;
mod router;
pub mod sse;

pub use adapter::{ModelAdapter, ModelCompletionRequest, ModelCompletionResponse, ModelStreamEvent};

pub use catalog::{ModelCatalog, ProviderModels, resolve_model_route};
pub use discovery::DiscoveredModel;
pub use openai::OpenAiClient;
pub use router::{ModelRouter, guard_route};

pub use crate::engine::compactor::llm::ModelHandle;

#[cfg(test)]
mod tests;
