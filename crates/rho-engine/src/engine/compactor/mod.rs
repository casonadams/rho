pub mod artifact_spill;
pub mod branch;
mod fit;
pub mod llm;
pub mod orchestrator;

#[cfg(test)]
mod tests;

pub use artifact_spill::{ArtifactDemotionHook, DemotionHook};

/// Times one turn of an agent may compact its context and carry on before it gives up.
pub(crate) const MAX_COMPACTIONS_PER_TURN: usize = 3;
pub use llm::LlmCompactor;
pub use orchestrator::{CompactionStats, SessionCompactor, compact_messages};
