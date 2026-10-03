use super::*;
use crate::adapter::rig::RigSessionMemory;
use rho_harness_core::session::SessionManager;
use rig::memory::Compactor;
use rig::message::{
    AssistantContent, CallId, Message, ToolCall, ToolFunction, ToolName, ToolResult, ToolResultContent, UserContent,
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

fn coding_tool_calls() -> Vec<AssistantContent> {
    vec![
        AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire("edit-call"),
            ToolFunction::new(
                ToolName::new("edit").unwrap(),
                serde_json::json!({"path":"src/lib.rs","edits":[]}),
            ),
        )),
        AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire("test-call"),
            ToolFunction::new(
                ToolName::new("bash").unwrap(),
                serde_json::json!({"command":"cargo test --all-targets"}),
            ),
        )),
    ]
}

fn coding_tool_results() -> Vec<UserContent> {
    vec![
        UserContent::ToolResult(ToolResult {
            call: CallId::from_wire("edit-call"),
            name: ToolName::new("edit").unwrap(),
            content: vec![ToolResultContent::text("changed")],
        }),
        UserContent::ToolResult(ToolResult {
            call: CallId::from_wire("test-call"),
            name: ToolName::new("bash").unwrap(),
            content: vec![ToolResultContent::text("tests passed")],
        }),
    ]
}

fn coding_turn(secret: Option<&str>) -> Vec<Message> {
    let objective = format!(
        "Objective: complete migration.\nConstraint: remain offline.\nDecision: use Rig memory.\nTests: cargo test passed.\nError: prior check failed.\nUnresolved work: finish evaluation.{}",
        secret.map_or(String::new(), |value| format!("\n{value}"))
    );
    vec![
        Message::user(objective),
        Message::Assistant {
            id: None,
            content: coding_tool_calls(),
        },
        Message::User {
            content: coding_tool_results(),
        },
        Message::assistant("Decision: retain recent rounds exactly."),
    ]
}

#[test]
fn sliding_window_exact() {
    let history = coding_turn(None);
    let exact = SlidingWindowMemory::last_messages(4).apply(history.clone()).unwrap();
    assert_eq!(exact, history);
}

fn is_tool_result_first(msgs: &[Message]) -> bool {
    msgs.first().is_some_and(
        |m| matches!(m, Message::User { content } if matches!(content.first(), Some(UserContent::ToolResult(_)))),
    )
}

#[test]
fn sliding_window_short() {
    let history = coding_turn(None);
    let window = SlidingWindowMemory::last_messages(2).apply(history.clone()).unwrap();
    assert_eq!(window, history[3..]);
    assert!(!is_tool_result_first(&window));
}

#[test]
fn sliding_window_preserves_tool_pair() {
    let mut long = simple_turn(0);
    long.extend(coding_turn(None));
    let complete_pair = SlidingWindowMemory::last_messages(3).apply(long).unwrap();
    assert_eq!(complete_pair.len(), 3);
    assert!(matches!(
        (&complete_pair[0], &complete_pair[1]),
        (Message::Assistant { .. }, Message::User { .. })
    ));
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
    let cid = rig::id::ConversationId::from(id.as_str());
    ConversationMemory::append(&RigSessionMemory::new(durable.clone()), &cid, history.clone())
        .await
        .unwrap();
    let memory = context_memory(durable.clone(), 4, 2048);
    let visible = memory.load(&cid).await.unwrap();

    let domain_history: Vec<_> = history.iter().map(crate::adapter::rig::from_rig_message).collect();
    assert_eq!(durable.load_messages().await.unwrap(), domain_history);
    assert_eq!(visible.len(), 5);
    assert!(matches!(visible.first(), Some(Message::System { .. })));
    assert_eq!(&visible[1..], &history[history.len() - 4..]);
    assert!(model_visible_bytes(&visible) < model_visible_bytes(&history));
    drop(durable);
    let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
    assert_eq!(resumed.load_messages().await.unwrap(), domain_history);
}

fn assert_required_artifact_fragments(artifact: &str) {
    let required_fragments = [
        "Objective: complete migration",
        "Constraint: remain offline",
        "Decision: use Rig memory",
        "changed file: src/lib.rs",
        "verification command: cargo test --all-targets",
        "tests passed",
        "Error: prior check failed",
        "Unresolved work: finish evaluation",
    ];
    for required in required_fragments {
        assert!(artifact.contains(required), "missing {required}");
    }
}

#[tokio::test]
async fn template_loss_justifies_coding_artifact_that_retains_required_state() {
    let dir = temp_dir("state");
    let durable = SessionManager::new(&dir, None).unwrap();
    let id = durable.session_id.clone();
    let history = coding_turn(None);
    let cid = rig::id::ConversationId::from(id.as_str());
    let template = TemplateCompactor::new().compact(&cid, &history, None).await.unwrap();
    assert!(!template.as_str().contains("src/lib.rs"));
    assert!(!template.as_str().contains("tests passed"));

    let compactor = CodingCompactor::new(durable, 4096);
    let artifact = Compactor::compact(&compactor, &cid, &history, None).await.unwrap();
    assert_required_artifact_fragments(artifact.as_str());
    assert!(artifact.as_str().len() <= 4096);
    let params = super::artifact::ArtifactParams {
        carry: None,
        messages: &history,
        template: template.as_str(),
        max_bytes: 1,
    };
    assert!(super::artifact::build_artifact(params).len() <= 1);
}

async fn assert_resumed_context(resumed: &SessionManager, first: &[Message]) {
    let resumed_memory = context_memory(resumed.clone(), 4, 4096);
    let cid = rig::id::ConversationId::from(resumed.session_id.as_str());
    assert_eq!(resumed_memory.load(&cid).await.unwrap(), first);
    let msgs = resumed.load_messages().await.unwrap();
    assert!(
        msgs.iter()
            .all(|m| !matches!(m, rho_harness_core::model::ChatMessage::System { .. }))
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
    let cid = rig::id::ConversationId::from(id.as_str());
    ConversationMemory::append(&RigSessionMemory::new(durable.clone()), &cid, history.clone())
        .await
        .unwrap();

    let memory = context_memory(durable.clone(), 4, 4096);
    let (first, second) = tokio::join!(memory.load(&cid), memory.load(&cid));
    let first = first.unwrap();
    assert_eq!(second.unwrap(), first);
    assert_eq!(&first[1..], &history[history.len() - 4..]);
    assert_eq!(first.iter().filter(|m| matches!(m, Message::System { .. })).count(), 1);

    drop(memory);
    drop(durable);
    let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
    assert_resumed_context(&resumed, &first).await;
}

struct FailingCompactor;

impl Compactor for FailingCompactor {
    type Artifact = CodingArtifact;

    fn compact<'a>(
        &'a self,
        _conversation_id: &'a rig::id::ConversationId,
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
    let cid = rig::id::ConversationId::from(id.as_str());
    ConversationMemory::append(&RigSessionMemory::new(durable.clone()), &cid, history.clone())
        .await
        .unwrap();
    let memory = CompactingMemory::new(
        RigSessionMemory::new(durable.clone()),
        SlidingWindowMemory::last_messages(2),
        FailingCompactor,
    );

    let error = memory.load(&cid).await.unwrap_err().to_string();
    assert!(error.contains("compaction unavailable"));
    let domain_history: Vec<_> = history.iter().map(crate::adapter::rig::from_rig_message).collect();
    assert_eq!(durable.load_messages().await.unwrap(), domain_history);
    let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
    assert_eq!(reopened.load_messages().await.unwrap(), domain_history);
}

#[tokio::test]
async fn compaction_artifact_and_sidecar_are_secret_free() {
    let dir = temp_dir("secret");
    let durable = SessionManager::new_with_secrets(&dir, None, vec!["credential-sentinel".to_string()]).unwrap();
    let id = durable.session_id.clone();
    let compactor = CodingCompactor::new(durable.clone(), 1024);
    let cid = rig::id::ConversationId::from(id.as_str());
    let artifact = Compactor::compact(&compactor, &cid, &coding_turn(Some("credential-sentinel")), None)
        .await
        .unwrap();
    assert!(!artifact.as_str().contains("credential-sentinel"));
    let sidecar = std::fs::read_to_string(durable.file_path.with_extension("context.json")).unwrap();
    assert!(!sidecar.contains("credential-sentinel"));
}
