use super::{SessionEventKind, SessionManager};
use rig::memory::ConversationMemory;
use rig::message::{
    AssistantContent, Message, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
};
use std::io::Write;
use std::path::PathBuf;

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("session_test_{}", uuid::Uuid::new_v4()))
}

fn complete_tool_turn(ids: &[&str]) -> Vec<Message> {
    let calls = ids
        .iter()
        .map(|id| {
            AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new(*id).unwrap(),
                ToolFunction::new("read".to_string(), serde_json::json!({"path": id})),
            ))
        })
        .collect();
    let results = ids
        .iter()
        .map(|id| {
            UserContent::ToolResult(ToolResult {
                call: ToolCallId::new(*id).unwrap(),
                provider: None,
                name: "read".to_string(),
                content: vec![ToolResultContent::text("ok")],
            })
        })
        .collect();
    vec![
        Message::user("read files"),
        Message::Assistant {
            id: None,
            content: calls,
        },
        Message::User { content: results },
        Message::assistant("done"),
    ]
}

#[cfg(unix)]
#[test]
fn session_storage_is_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(
        std::fs::metadata(&store.file_path).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let id = store.session_id.clone();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&store.file_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    drop(store);
    let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(
        std::fs::metadata(&resumed.file_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[tokio::test]
async fn empty_v2_session_round_trips_after_reopen() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    assert!(store.load_messages().await.unwrap().is_empty());
    drop(store);
    let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
    assert!(reopened.load_messages().await.unwrap().is_empty());
}

#[test]
fn resume_requires_supported_versioned_header() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    for (id, body, expected) in [
        ("legacy", r#"{"id":"old"}\n"#, "legacy session"),
        (
            "missing",
            r#"{"record_type":"header","session_id":"missing"}\n"#,
            "mandatory version",
        ),
        (
            "wrong",
            r#"{"record_type":"header","version":1,"session_id":"wrong","created_at":"2025-01-01T00:00:00Z"}\n"#,
            "unsupported session version",
        ),
        (
            "future",
            r#"{"record_type":"header","version":3,"session_id":"future","created_at":"2025-01-01T00:00:00Z"}\n"#,
            "unsupported session version",
        ),
    ] {
        std::fs::write(dir.join(format!("{id}.jsonl")), body.replace("\\n", "\n")).unwrap();
        let error = SessionManager::new(&dir, Some(id)).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn missing_and_unknown_sessions_fail_clearly() {
    let dir = temp_dir();
    let missing = SessionManager::new(&dir, Some("unknown")).unwrap_err().to_string();
    assert!(missing.contains("unknown session id"));
    std::fs::write(dir.join("empty.jsonl"), "").unwrap();
    let empty = SessionManager::new(&dir, Some("empty")).unwrap_err().to_string();
    assert!(empty.contains("mandatory version header"));
}

#[tokio::test]
async fn canonical_memory_round_trips_multi_turn_and_multi_tool_order() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    ConversationMemory::append(&store, &id, complete_tool_turn(&["call-1", "call-2"]))
        .await
        .unwrap();
    ConversationMemory::append(&store, &id, vec![Message::user("next"), Message::assistant("answer")])
        .await
        .unwrap();
    let expected = ConversationMemory::load(&store, &id).await.unwrap();
    drop(store);
    let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
    assert_eq!(ConversationMemory::load(&reopened, &id).await.unwrap(), expected);
}

#[tokio::test]
async fn rejects_orphan_dangling_and_miscorrelated_tools() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    let mut dangling = complete_tool_turn(&["call-1"]);
    dangling.truncate(2);
    assert!(ConversationMemory::append(&store, &id, dangling).await.is_err());

    let orphan = vec![complete_tool_turn(&["call-1"])[2].clone(), Message::assistant("done")];
    assert!(ConversationMemory::append(&store, &id, orphan).await.is_err());

    let mut wrong = complete_tool_turn(&["call-1"]);
    if let Message::User { content } = &mut wrong[2]
        && let UserContent::ToolResult(result) = &mut content[0]
    {
        result.call = ToolCallId::new("other").unwrap();
    }
    assert!(ConversationMemory::append(&store, &id, wrong).await.is_err());
    assert!(store.load_messages().await.unwrap().is_empty());
}

#[tokio::test]
async fn memory_identity_failures_do_not_change_history() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    assert!(ConversationMemory::load(&store, "wrong-id").await.is_err());
    assert!(
        ConversationMemory::append(
            &store,
            "wrong-id",
            vec![Message::user("prompt"), Message::assistant("answer")],
        )
        .await
        .is_err()
    );
    assert!(store.load_messages().await.unwrap().is_empty());
    let error = store.take_memory_error().unwrap();
    assert!(error.contains("identity mismatch"));
}

#[tokio::test]
async fn clear_preserves_file_and_audit_but_starts_fresh_history() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    ConversationMemory::append(&store, &id, vec![Message::user("old"), Message::assistant("answer")])
        .await
        .unwrap();
    store
        .append_event(
            SessionEventKind::AssistantResponse,
            serde_json::json!({"status":"complete"}),
        )
        .await
        .unwrap();
    ConversationMemory::clear(&store, &id).await.unwrap();
    assert!(ConversationMemory::load(&store, &id).await.unwrap().is_empty());
    assert_eq!(store.load_events().await.unwrap().len(), 1);
    let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
    assert!(reopened.load_messages().await.unwrap().is_empty());
    assert_eq!(reopened.load_events().await.unwrap().len(), 1);
}

#[tokio::test]
async fn concurrent_appends_are_serialized_without_interleaving() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let tasks = (0..20).map(|index| {
        let store = store.clone();
        tokio::spawn(async move {
            store
                .append_event(SessionEventKind::UsageMetrics, serde_json::json!({"index":index}))
                .await
                .unwrap();
        })
    });
    futures::future::join_all(tasks).await;
    let reopened = SessionManager::new(&dir, Some(&store.session_id)).unwrap();
    assert_eq!(reopened.load_events().await.unwrap().len(), 20);
}

#[tokio::test]
async fn budget_checkpoint_resumes_and_promotes_atomically_after_success() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    ConversationMemory::append(
        &store,
        &id,
        vec![Message::user("earlier"), Message::assistant("answer")],
    )
    .await
    .unwrap();
    let mut checkpoint = complete_tool_turn(&["call-1", "call-2"]);
    checkpoint.pop();
    store.save_checkpoint(checkpoint.clone()).await.unwrap();
    assert_eq!(store.load_messages().await.unwrap().len(), 2);
    assert_eq!(store.load_checkpoint().await.unwrap(), Some(checkpoint.clone()));
    assert!(
        ConversationMemory::append(&store, &id, vec![Message::user("must wait"), Message::assistant("no")],)
            .await
            .is_err()
    );

    drop(store);
    let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
    assert_eq!(resumed.load_checkpoint().await.unwrap(), Some(checkpoint.clone()));
    resumed
        .promote_checkpoint(vec![Message::user("please continue"), Message::assistant("done")])
        .await
        .unwrap();
    assert!(resumed.load_checkpoint().await.unwrap().is_none());
    assert_eq!(resumed.load_messages().await.unwrap().len(), 7);

    drop(resumed);
    let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
    assert!(reopened.load_checkpoint().await.unwrap().is_none());
    assert_eq!(reopened.load_messages().await.unwrap().len(), 7);
}

#[tokio::test]
async fn budget_checkpoint_rejects_dangling_tools_and_credentials() {
    let dir = temp_dir();
    let store = SessionManager::new_with_secrets(&dir, None, vec!["credential-sentinel".to_string()]).unwrap();
    let mut dangling = complete_tool_turn(&["call-1"]);
    dangling.truncate(2);
    assert!(store.save_checkpoint(dangling).await.is_err());
    let error = store
        .save_checkpoint(vec![Message::user("credential-sentinel")])
        .await
        .unwrap_err()
        .to_string();
    assert!(!error.contains("credential-sentinel"));
    assert!(store.load_checkpoint().await.unwrap().is_none());
    assert!(
        !std::fs::read_to_string(&store.file_path)
            .unwrap()
            .contains("credential-sentinel")
    );
}

#[tokio::test]
async fn cancellation_fixtures_remain_parseable_and_resumable() {
    for boundary in [
        "before_first_token",
        "during_text",
        "between_call_result",
        "during_tool",
    ] {
        let dir = temp_dir();
        let store = SessionManager::new(&dir, None).unwrap();
        let id = store.session_id.clone();
        store
            .append_event(
                SessionEventKind::Cancellation,
                serde_json::json!({"boundary": boundary, "terminal": true}),
            )
            .await
            .unwrap();
        drop(store);

        let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
        assert!(reopened.load_messages().await.unwrap().is_empty());
        ConversationMemory::append(
            &reopened,
            &id,
            vec![Message::user("after cancel"), Message::assistant("resumed")],
        )
        .await
        .unwrap();
        drop(reopened);
        let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
        assert_eq!(resumed.load_messages().await.unwrap().len(), 2);
    }
}

#[test]
fn malformed_committed_records_fail_but_incomplete_tail_is_ignored() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&store.file_path)
        .unwrap()
        .write_all(b"{interrupted")
        .unwrap();
    assert!(SessionManager::new(&dir, Some(&id)).is_ok());

    let bad_dir = temp_dir();
    let bad = SessionManager::new(&bad_dir, None).unwrap();
    let bad_id = bad.session_id.clone();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&bad.file_path)
        .unwrap()
        .write_all(b"{malformed}\n")
        .unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&bad.file_path)
        .unwrap()
        .write_all(b"{\"record_type\":\"canonical_reset\",\"sequence\":1,\"session_id\":\"ignored\",\"timestamp\":\"2025-01-01T00:00:00Z\"}\n")
        .unwrap();
    let error = SessionManager::new(&bad_dir, Some(&bad_id)).unwrap_err().to_string();
    assert!(error.contains("malformed committed record"));
}

#[tokio::test]
async fn credential_values_are_rejected_without_persistence_or_error_echo() {
    let dir = temp_dir();
    let store = SessionManager::new_with_secrets(&dir, None, vec!["credential-sentinel".to_string()]).unwrap();
    assert_eq!(
        store.redact_credentials("prefix credential-sentinel suffix"),
        "prefix [REDACTED] suffix"
    );
    let error = store
        .append_event(
            SessionEventKind::UserMessage,
            serde_json::json!({"text":"credential-sentinel"}),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(!error.contains("credential-sentinel"));
    let persisted = std::fs::read_to_string(&store.file_path).unwrap();
    assert!(!persisted.contains("credential-sentinel"));
}
