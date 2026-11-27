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

use crate::engine::tracking::CompactionTiming;
use rho_harness_core::error::AppError;

/// Times one turn of an agent may compact its context and carry on before it gives up.
const MAX_COMPACTIONS_PER_TURN: usize = 3;

/// How a turn that stopped for a full context goes on.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MadeRoom<T> {
    /// Its context was compacted into `T`.
    Compacted(T),
    /// Nothing could be compacted before an overflow, so it runs on until one.
    RunOn,
}

/// One turn's compactions so far and when it next stops to compact; shared by every agent loop.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TurnCompaction {
    compactions: usize,
    timing: CompactionTiming,
}

impl Default for TurnCompaction {
    fn default() -> Self {
        Self {
            compactions: 0,
            timing: CompactionTiming::Early,
        }
    }
}

impl TurnCompaction {
    pub(crate) fn timing(self) -> CompactionTiming {
        self.timing
    }

    pub(crate) fn has_compacted(self) -> bool {
        self.compactions > 0
    }

    /// Makes room after `error` stopped the turn: compacts with `compact` while compactions are
    /// left, else runs on when the stop came before an overflow. `None` when the turn cannot go on.
    pub(crate) async fn make_room<T, E, F>(&mut self, error: &AppError, compact: F) -> Result<Option<MadeRoom<T>>, E>
    where
        F: AsyncFnOnce() -> Result<Option<T>, E>,
    {
        if !error.needs_compaction() {
            return Ok(None);
        }
        if self.compactions < MAX_COMPACTIONS_PER_TURN
            && let Some(compacted) = compact().await?
        {
            self.compactions += 1;
            self.timing = CompactionTiming::Early;
            return Ok(Some(MadeRoom::Compacted(compacted)));
        }
        if !error.is_early_stop() {
            return Ok(None);
        }
        self.timing = CompactionTiming::OnOverflow;
        Ok(Some(MadeRoom::RunOn))
    }
}
