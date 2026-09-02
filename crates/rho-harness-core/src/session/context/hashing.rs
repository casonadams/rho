//! Pure helpers for compacting evicted conversation messages.
//!
//! Extracted from `session/context.rs` during the file-length refactor.

use rig::memory::MemoryError;
use rig::message::Message;

pub(super) fn message_hashes(messages: &[Message]) -> Result<Vec<String>, MemoryError> {
    messages
        .iter()
        .map(|message| {
            let bytes = serde_json::to_vec(message)
                .map_err(|_| MemoryError::Internal("canonical message hashing failed".to_string()))?;
            let hash = bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
            Ok(format!("{hash:016x}"))
        })
        .collect()
}
