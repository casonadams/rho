//! Pure helpers for compacting evicted conversation messages.
//!
//! Extracted from `session/context.rs` during the file-length refactor.

use rig::memory::MemoryError;
use rig::message::Message;

struct Fnv1aWriter(u64);

impl std::io::Write for Fnv1aWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for &byte in buf {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn message_hashes(messages: &[Message]) -> Result<Vec<String>, MemoryError> {
    messages
        .iter()
        .map(|message| {
            let mut writer = Fnv1aWriter(0xcbf29ce484222325_u64);
            serde_json::to_writer(&mut writer, message)
                .map_err(|_| MemoryError::Internal("canonical message hashing failed".to_string()))?;
            Ok(format!("{:016x}", writer.0))
        })
        .collect()
}
