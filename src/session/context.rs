use super::SessionManager;
use rig::memory::{Compactor, ConversationMemory, MemoryError};
use rig::message::{AssistantContent, Message, ToolResultContent, UserContent};
use rig_memory::{CompactingMemory, SlidingWindowMemory, TemplateCompactor};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

pub const DEFAULT_CONTEXT_WINDOW_MESSAGES: usize = 24;
pub const DEFAULT_COMPACTION_MAX_BYTES: usize = 8 * 1024;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompactionState {
    version: u32,
    absorbed_hashes: Vec<String>,
    artifact: String,
}

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

    fn state_path(&self) -> PathBuf {
        self.session.file_path.with_extension("context.json")
    }

    fn load_state(&self) -> Result<Option<CompactionState>, MemoryError> {
        match std::fs::read(self.state_path()) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| MemoryError::Internal("stored compaction state is malformed".to_string())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(MemoryError::backend(error)),
        }
    }

    fn persist_state(&self, state: &CompactionState) -> Result<(), MemoryError> {
        let path = self.state_path();
        let temporary = path.with_extension(format!("context.{}.tmp", uuid::Uuid::new_v4()));
        let bytes = serde_json::to_vec(state)
            .map_err(|_| MemoryError::Internal("compaction state serialization failed".to_string()))?;
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(MemoryError::backend)?;
        file.write_all(&bytes).map_err(MemoryError::backend)?;
        file.sync_data().map_err(MemoryError::backend)?;
        std::fs::rename(temporary, path).map_err(MemoryError::backend)
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
            let template = self.template.compact("rust-ai", &template_input, None).await?;
            let artifact = build_artifact(ArtifactParams {
                carry: carry.as_deref(),
                messages: new_messages,
                template: template.as_str(),
                max_bytes: self.max_bytes,
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

struct CompactionInputParams<'a> {
    stored: Option<&'a CompactionState>,
    carry_over: Option<&'a CodingArtifact>,
    evicted: &'a [Message],
    hashes: &'a [String],
}

fn compaction_input<'a>(params: CompactionInputParams<'a>) -> (Option<String>, &'a [Message], Vec<String>) {
    if let Some(carry) = params.carry_over {
        let absorbed = params
            .stored
            .filter(|state| state.artifact == carry.as_str())
            .map_or_else(
                || params.hashes.to_vec(),
                |state| {
                    let mut combined = state.absorbed_hashes.clone();
                    combined.extend(params.hashes.iter().cloned());
                    combined
                },
            );
        return (Some(carry.as_str().to_string()), params.evicted, absorbed);
    }
    if let Some(state) = params.stored
        && params.hashes.starts_with(&state.absorbed_hashes)
    {
        let suffix = params.evicted.get(state.absorbed_hashes.len()..).unwrap_or_default();
        return (Some(state.artifact.clone()), suffix, params.hashes.to_vec());
    }
    (None, params.evicted, params.hashes.to_vec())
}

fn message_hashes(messages: &[Message]) -> Result<Vec<String>, MemoryError> {
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

struct ArtifactParams<'a> {
    carry: Option<&'a str>,
    messages: &'a [Message],
    template: &'a str,
    max_bytes: usize,
}

fn build_artifact(params: ArtifactParams<'_>) -> String {
    let facts = critical_facts(params.carry, params.messages);
    let mut output = String::new();
    push_bounded_line(&mut output, "[Coding context summary]", params.max_bytes);
    for fact in facts {
        push_bounded_line(&mut output, &fact, params.max_bytes);
    }
    push_bounded_line(&mut output, "Recent compacted transcript:", params.max_bytes);
    append_bounded_suffix(&mut output, params.template, params.max_bytes);
    output
}

fn critical_facts(carry: Option<&str>, messages: &[Message]) -> Vec<String> {
    let mut facts = Vec::new();
    let mut seen = HashSet::new();
    if let Some(previous) = carry {
        for line in previous.lines().filter(|line| is_critical_text(line)) {
            insert_fact(&mut facts, &mut seen, line.trim().to_string());
        }
    }
    for message in messages {
        collect_message_facts(message, &mut facts, &mut seen);
    }
    facts
}

fn collect_message_facts(message: &Message, facts: &mut Vec<String>, seen: &mut HashSet<String>) {
    match message {
        Message::User { content } => {
            for part in content {
                match part {
                    UserContent::Text(text) => collect_text_facts(&text.text, facts, seen),
                    UserContent::ToolResult(result) => {
                        for content in &result.content {
                            let ToolResultContent::Text(text) = content else {
                                continue;
                            };
                            if result.name == "bash" || is_error_text(&text.text) {
                                insert_fact(
                                    facts,
                                    seen,
                                    format!("tool result ({}): {}", result.name, text.text.trim()),
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Message::Assistant { content, .. } => {
            for part in content {
                match part {
                    AssistantContent::Text(text) => collect_text_facts(&text.text, facts, seen),
                    AssistantContent::ToolCall(call) => collect_tool_call_facts(call, facts, seen),
                    _ => {}
                }
            }
        }
        Message::System { content } => collect_text_facts(content, facts, seen),
    }
}

fn collect_tool_call_facts(call: &rig::message::ToolCall, facts: &mut Vec<String>, seen: &mut HashSet<String>) {
    match call.function.name.as_str() {
        "write" | "edit" => {
            if let Some(path) = call.function.arguments.get("path").and_then(serde_json::Value::as_str) {
                insert_fact(facts, seen, format!("changed file: {path}"));
            }
        }
        "bash" => {
            if let Some(command) = call
                .function
                .arguments
                .get("command")
                .and_then(serde_json::Value::as_str)
            {
                insert_fact(facts, seen, format!("verification command: {command}"));
            }
        }
        _ => {}
    }
}

fn collect_text_facts(text: &str, facts: &mut Vec<String>, seen: &mut HashSet<String>) {
    for line in text.lines().filter(|line| is_critical_text(line)) {
        insert_fact(facts, seen, line.trim().to_string());
    }
}

fn insert_fact(facts: &mut Vec<String>, seen: &mut HashSet<String>, fact: String) {
    if !fact.is_empty() && seen.insert(fact.clone()) {
        facts.push(fact);
    }
}

fn is_critical_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "objective",
        "constraint",
        "decision",
        "changed file",
        "verification",
        "test",
        "error",
        "failed",
        "unresolved",
        "remaining",
        "todo",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn is_error_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    ["error", "failed", "denied", "timed out", "not found"]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn push_bounded_line(output: &mut String, line: &str, max_bytes: usize) {
    if output.len() >= max_bytes {
        return;
    }
    let available = max_bytes.saturating_sub(output.len() + 1);
    let end = char_boundary_at_most(line, available);
    output.push_str(line.get(..end).unwrap_or_default());
    output.push('\n');
}

fn append_bounded_suffix(output: &mut String, text: &str, max_bytes: usize) {
    let available = max_bytes.saturating_sub(output.len());
    if available == 0 {
        return;
    }
    let start = char_boundary_at_least(text, text.len().saturating_sub(available));
    output.push_str(text.get(start..).unwrap_or_default());
}

fn char_boundary_at_most(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn char_boundary_at_least(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

pub fn model_visible_bytes(messages: &[Message]) -> usize {
    serde_json::to_vec(messages).map_or(0, |bytes| bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::message::{ToolCall, ToolCallId, ToolFunction, ToolResult};
    use rig_memory::MemoryPolicy;

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

        let artifact = CodingCompactor::new(durable, 4096)
            .compact(&id, &history, None)
            .await
            .unwrap();
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
            build_artifact(ArtifactParams {
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
        let artifact = CodingCompactor::new(durable.clone(), 1024)
            .compact(&id, &coding_turn(Some("credential-sentinel")), None)
            .await
            .unwrap();
        assert!(!artifact.as_str().contains("credential-sentinel"));
        let sidecar = std::fs::read_to_string(durable.file_path.with_extension("context.json")).unwrap();
        assert!(!sidecar.contains("credential-sentinel"));
    }
}
