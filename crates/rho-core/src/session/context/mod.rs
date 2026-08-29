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
mod tests {
    use super::*;
    use rig::memory::Compactor;
    use rig::message::{
        AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
    };
    use rig_memory::{
        CompactingMemory, ConversationMemory, MemoryError, MemoryPolicy, SlidingWindowMemory, TemplateCompactor,
    };
    use std::path::PathBuf;

    fn temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("context_{label}_{}", uuid::Uuid::new_v4()))
    }

    fn simple_turn(index: usize) -> Vec<Message> {
        vec![
            Message::user(format!("prompt {index}: {}", "input ".repeat(20))),
            Message::assistant(format!("answer {index}: {}", "output ".repeat(20))),
        ]
    }

    fn coding_turn(secret: Option<&str>) -> Vec<Message> {
        let objective = format!(
            "Objective: complete migration.\nConstraint: remain offline.\nDecision: use Rig memory.\nTests: cargo test passed.\nError: prior check failed.\nUnresolved work: finish evaluation.{}",
            secret.map_or(String::new(), |value| format!("\n{value}"))
        );
        let calls = vec![
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new("edit-call").unwrap(),
                ToolFunction::new("edit".to_string(), serde_json::json!({"path":"src/lib.rs","edits":[]})),
            )),
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new("test-call").unwrap(),
                ToolFunction::new(
                    "bash".to_string(),
                    serde_json::json!({"command":"cargo test --all-targets"}),
                ),
            )),
        ];
        let results = vec![
            UserContent::ToolResult(ToolResult {
                call: ToolCallId::new("edit-call").unwrap(),
                provider: None,
                name: "edit".to_string(),
                content: vec![ToolResultContent::text("changed")],
            }),
            UserContent::ToolResult(ToolResult {
                call: ToolCallId::new("test-call").unwrap(),
                provider: None,
                name: "bash".to_string(),
                content: vec![ToolResultContent::text("tests passed")],
            }),
        ];
        vec![
            Message::user(objective),
            Message::Assistant {
                id: None,
                content: calls,
            },
            Message::User { content: results },
            Message::assistant("Decision: retain recent rounds exactly."),
        ]
    }

    #[test]
    fn sliding_window_exact_boundary_and_orphan_protection_preserve_tool_batches() {
        let history = coding_turn(None);
        let exact = SlidingWindowMemory::last_messages(4).apply(history.clone()).unwrap();
        assert_eq!(exact, history);

        let window = SlidingWindowMemory::last_messages(2).apply(history.clone()).unwrap();
        assert_eq!(window, history[3..]);
        assert!(
            !matches!(window.first(), Some(Message::User { content }) if matches!(content.first(), Some(UserContent::ToolResult(_))))
        );

        let mut long = simple_turn(0);
        long.extend(coding_turn(None));
        let complete_pair = SlidingWindowMemory::last_messages(3).apply(long).unwrap();
        assert_eq!(complete_pair.len(), 3);
        assert!(matches!(complete_pair[0], Message::Assistant { .. }));
        assert!(matches!(complete_pair[1], Message::User { .. }));
    }

    #[tokio::test]
    async fn durable_history_remains_full_while_model_history_is_compacted_and_bounded() {
        let dir = temp_dir("durable");
        let durable = SessionManager::new(&dir, None).unwrap();
        let id = durable.session_id.clone();
        let mut history = coding_turn(None);
        for index in 0..8 {
            history.extend(simple_turn(index));
        }
        ConversationMemory::append(&durable, &id, history.clone())
            .await
            .unwrap();
        let memory = context_memory(durable.clone(), 4, 2048);
        let visible = memory.load(&id).await.unwrap();

        assert_eq!(durable.load_messages().await.unwrap(), history);
        assert_eq!(visible.len(), 5);
        assert!(matches!(visible.first(), Some(Message::System { .. })));
        assert_eq!(&visible[1..], &history[history.len() - 4..]);
        assert!(model_visible_bytes(&visible) < model_visible_bytes(&history));
        drop(durable);
        let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
        assert_eq!(resumed.load_messages().await.unwrap(), history);
    }

    #[tokio::test]
    async fn template_loss_justifies_coding_artifact_that_retains_required_state() {
        let dir = temp_dir("state");
        let durable = SessionManager::new(&dir, None).unwrap();
        let id = durable.session_id.clone();
        let history = coding_turn(None);
        let template = TemplateCompactor::new().compact(&id, &history, None).await.unwrap();
        assert!(!template.as_str().contains("src/lib.rs"));
        assert!(!template.as_str().contains("tests passed"));

        let compactor = CodingCompactor::new(durable, 4096);
        let artifact = Compactor::compact(&compactor, &id, &history, None).await.unwrap();
        for required in [
            "Objective: complete migration",
            "Constraint: remain offline",
            "Decision: use Rig memory",
            "changed file: src/lib.rs",
            "verification command: cargo test --all-targets",
            "tests passed",
            "Error: prior check failed",
            "Unresolved work: finish evaluation",
        ] {
            assert!(artifact.as_str().contains(required), "missing {required}");
        }
        assert!(artifact.as_str().len() <= 4096);
        assert!(
            super::artifact::build_artifact(super::artifact::ArtifactParams {
                carry: None,
                messages: &history,
                template: template.as_str(),
                max_bytes: 1,
            })
            .len()
                <= 1
        );
    }

    #[tokio::test]
    async fn recent_rounds_restart_deduplication_and_concurrent_loads_are_stable() {
        let dir = temp_dir("resume");
        let durable = SessionManager::new(&dir, None).unwrap();
        let id = durable.session_id.clone();
        let mut history = coding_turn(None);
        history.extend(simple_turn(1));
        history.extend(simple_turn(2));
        ConversationMemory::append(&durable, &id, history.clone())
            .await
            .unwrap();
        let memory = context_memory(durable.clone(), 4, 4096);
        let (first, second) = tokio::join!(memory.load(&id), memory.load(&id));
        let first = first.unwrap();
        assert_eq!(second.unwrap(), first);
        assert_eq!(&first[1..], &history[history.len() - 4..]);
        assert_eq!(
            first
                .iter()
                .filter(|message| matches!(message, Message::System { .. }))
                .count(),
            1
        );
        let persisted = std::fs::read_to_string(durable.file_path.with_extension("context.json")).unwrap();

        drop(memory);
        drop(durable);
        let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
        let resumed_memory = context_memory(resumed.clone(), 4, 4096);
        assert_eq!(resumed_memory.load(&id).await.unwrap(), first);
        assert_eq!(
            std::fs::read_to_string(resumed.file_path.with_extension("context.json")).unwrap(),
            persisted
        );
        assert!(
            resumed
                .load_messages()
                .await
                .unwrap()
                .iter()
                .all(|message| !matches!(message, Message::System { .. }))
        );
    }

    struct FailingCompactor;

    impl Compactor for FailingCompactor {
        type Artifact = CodingArtifact;

        fn compact<'a>(
            &'a self,
            _conversation_id: &'a str,
            _evicted: &'a [Message],
            _carry_over: Option<&'a Self::Artifact>,
        ) -> rig::wasm_compat::WasmBoxedFuture<'a, Result<Self::Artifact, MemoryError>> {
            Box::pin(async { Err(MemoryError::Policy("compaction unavailable".to_string())) })
        }
    }

    #[tokio::test]
    async fn compaction_failure_surfaces_without_changing_valid_canonical_history() {
        let dir = temp_dir("failure");
        let durable = SessionManager::new(&dir, None).unwrap();
        let id = durable.session_id.clone();
        let mut history = simple_turn(0);
        history.extend(simple_turn(1));
        ConversationMemory::append(&durable, &id, history.clone())
            .await
            .unwrap();
        let memory = CompactingMemory::new(durable.clone(), SlidingWindowMemory::last_messages(2), FailingCompactor);

        let error = memory.load(&id).await.unwrap_err().to_string();
        assert!(error.contains("compaction unavailable"));
        assert_eq!(durable.load_messages().await.unwrap(), history);
        let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
        assert_eq!(reopened.load_messages().await.unwrap(), history);
    }

    #[tokio::test]
    async fn compaction_artifact_and_sidecar_are_secret_free() {
        let dir = temp_dir("secret");
        let durable = SessionManager::new_with_secrets(&dir, None, vec!["credential-sentinel".to_string()]).unwrap();
        let id = durable.session_id.clone();
        let compactor = CodingCompactor::new(durable.clone(), 1024);
        let artifact = Compactor::compact(&compactor, &id, &coding_turn(Some("credential-sentinel")), None)
            .await
            .unwrap();
        assert!(!artifact.as_str().contains("credential-sentinel"));
        let sidecar = std::fs::read_to_string(durable.file_path.with_extension("context.json")).unwrap();
        assert!(!sidecar.contains("credential-sentinel"));
    }
}
