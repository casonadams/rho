use super::{SessionEventKind, SessionManager, complete_tool_turn, temp_dir};
use rig::memory::ConversationMemory;
use rig::message::{Message, ToolCallId, UserContent};

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
