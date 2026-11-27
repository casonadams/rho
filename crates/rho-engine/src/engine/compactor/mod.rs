pub mod artifact_spill;
pub mod branch;
mod fit;
pub mod llm;
pub mod orchestrator;

#[cfg(test)]
mod tests;

pub use artifact_spill::{ArtifactDemotionHook, DemotionHook};
pub use llm::LlmCompactor;
pub use orchestrator::{CompactionStats, SessionCompactor, compact_messages};

/// Times one turn of an agent may compact its context and carry on before it gives up.
const MAX_COMPACTIONS_PER_TURN: usize = 3;

/// Whether an agent that already compacted `compactions` times in one turn may do so again.
pub(crate) fn may_compact_again(compactions: usize) -> bool {
    compactions < MAX_COMPACTIONS_PER_TURN
}
