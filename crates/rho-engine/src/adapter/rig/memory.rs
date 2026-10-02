use crate::adapter::rig::{from_rig_message, into_rig_message};
use rho_harness_core::session::SessionManager;
use rig::memory::{ConversationMemory, MemoryError};
use rig::message::Message;

/// Adapter bridging Rho's `SessionManager` to Rig's `ConversationMemory`.
#[derive(Clone, Debug)]
pub struct RigSessionMemory {
    session: SessionManager,
}

impl RigSessionMemory {
    pub fn new(session: SessionManager) -> Self {
        Self { session }
    }

    pub fn session(&self) -> &SessionManager {
        &self.session
    }
}

impl std::ops::Deref for RigSessionMemory {
    type Target = SessionManager;

    fn deref(&self) -> &Self::Target {
        &self.session
    }
}

impl ConversationMemory for RigSessionMemory {
    fn load<'a>(
        &'a self,
        conversation_id: &'a str,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, std::result::Result<Vec<Message>, MemoryError>> {
        Box::pin(async move {
            if let Err(error) = self.session.ensure_conversation(conversation_id) {
                self.session.remember_memory_error(&error);
                return Err(MemoryError::backend(error));
            }
            let messages = self.session.load_messages().await.map_err(|error| {
                self.session.remember_memory_error(&error);
                MemoryError::backend(error)
            })?;
            Ok(messages.into_iter().map(into_rig_message).collect())
        })
    }

    fn append<'a>(
        &'a self,
        conversation_id: &'a str,
        messages: Vec<Message>,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, std::result::Result<(), MemoryError>> {
        Box::pin(async move {
            let rho_messages = messages.iter().map(from_rig_message).collect();
            self.session
                .append_messages(conversation_id, rho_messages)
                .await
                .map_err(|error| {
                    self.session.remember_memory_error(&error);
                    MemoryError::backend(error)
                })
        })
    }

    fn clear<'a>(
        &'a self,
        conversation_id: &'a str,
    ) -> rig::wasm_compat::WasmBoxedFuture<'a, std::result::Result<(), MemoryError>> {
        Box::pin(async move {
            self.session.clear_messages(conversation_id).await.map_err(|error| {
                self.session.remember_memory_error(&error);
                MemoryError::backend(error)
            })
        })
    }
}
