//! Conversation-memory compaction pipeline for the session.
//!
//! Originally a single 599-line module; split into focused submodules during
//! the file-length refactor.
//!
//! - [`compactor`] — `CodingCompactor` + `CodingArtifact` + `Compactor` impl
//!   and the `context_memory` / `model_visible_bytes` entry points.
//! - [`state`] — on-disk sidecar (`context.json`) persistence and the
//!   `compaction_input` helper that decides carry-forward behaviour.
//! - [`artifact`] — `build_artifact` and the critical-fact extraction that
//!   produces the summary body.
//! - [`hashing`] — FNV-1a message hashing for deduplication.

use super::SessionManager;

mod artifact;
mod compactor;
mod hashing;
mod state;

pub use compactor::{CodingArtifact, CodingCompactor, context_memory, model_visible_bytes};

pub const DEFAULT_CONTEXT_WINDOW_MESSAGES: usize = 24;
pub const DEFAULT_COMPACTION_MAX_BYTES: usize = 8 * 1024;

#[cfg(test)]
mod tests;
