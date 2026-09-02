//! `CodingCompactor`: implements rig's [`Compactor`] trait for the session
//! memory pipeline, including deduplication via sidecar state.
//!
//! Extracted from `session/context.rs` during the file-length refactor.

use std::sync::Arc;

use rig::memory::{Compactor, ConversationMemory};
use rig::message::Message;
use rig_memory::{CompactingMemory, MemoryError, SlidingWindowMemory, TemplateCompactor};

use super::SessionManager;
use super::artifact::{ArtifactParams, build_artifact};
use super::hashing::message_hashes;
use super::state::{CompactionInputParams, CompactionState, compaction_input};

#[derive(Debug, Clone)]
pub struct CodingArtifact(String);

impl CodingArtifact {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<CodingArtifact> for Message {
    fn from(artifact: CodingArtifact) -> Self {
        Message::System { content: artifact.0 }
    }
}

#[derive(Debug, Clone)]
pub struct CodingCompactor {
    session: SessionManager,
    max_bytes: usize,
    template: TemplateCompactor,
}

impl CodingCompactor {
    pub fn new(session: SessionManager, max_bytes: usize) -> Self {
        Self {
            session,
            max_bytes,
            template: TemplateCompactor::with_header("[Earlier canonical context]")
                .with_max_bytes(max_bytes.saturating_div(2).max(1)),
        }
    }

    pub(super) fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    pub(crate) fn session(&self) -> &SessionManager {
        &self.session
    }
}

impl Compactor for CodingCompactor {
    type Artifact = CodingArtifact;

    fn compact<'a>(
        &'a self,
        _conversation_id: &'a str,
        evicted: &'a [Message],
        carry_over: Option<&'a Self::Artifact>,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, Result<Self::Artifact, MemoryError>> {
        Box::pin(async move {
            let hashes = message_hashes(evicted)?;
            let stored = self.load_state()?;
            if carry_over.is_none()
                && stored
                    .as_ref()
                    .is_some_and(|state| state.version == 1 && state.absorbed_hashes == hashes)
            {
                let artifact = stored.map(|state| state.artifact).unwrap_or_default();
                return Ok(CodingArtifact(artifact));
            }

            let (carry, new_messages, absorbed_hashes) = compaction_input(CompactionInputParams {
                stored: stored.as_ref(),
                carry_over,
                evicted,
                hashes: &hashes,
            });
            let mut template_input = Vec::with_capacity(new_messages.len() + usize::from(carry.is_some()));
            if let Some(previous) = carry.as_ref() {
                template_input.push(Message::System {
                    content: previous.clone(),
                });
            }
            template_input.extend(new_messages.iter().cloned());
            let template = self.template.compact("rho", &template_input, None).await?;
            let artifact = build_artifact(ArtifactParams {
                carry: carry.as_deref(),
                messages: new_messages,
                template: template.as_str(),
                max_bytes: self.max_bytes(),
            });
            let artifact = self.session.redact_credentials(&artifact);
            self.persist_state(&CompactionState {
                version: 1,
                absorbed_hashes,
                artifact: artifact.clone(),
            })?;
            Ok(CodingArtifact(artifact))
        })
    }
}

pub fn context_memory(
    durable: SessionManager,
    window_messages: usize,
    compaction_max_bytes: usize,
) -> Arc<dyn ConversationMemory> {
    let compactor = CodingCompactor::new(durable.clone(), compaction_max_bytes);
    Arc::new(CompactingMemory::new(
        durable,
        SlidingWindowMemory::last_messages(window_messages),
        compactor,
    ))
}

pub fn model_visible_bytes(messages: &[Message]) -> usize {
    serde_json::to_vec(messages).map_or(0, |bytes| bytes.len())
}
