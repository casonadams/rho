use super::{SessionEventKind, SessionManager, complete_tool_turn, temp_dir};
use crate::model::{ChatMessage, UserContent};

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
    store
        .append_messages(&id, complete_tool_turn(&["call-1", "call-2"]))
        .await
        .unwrap();
    store
        .append_messages(&id, vec![ChatMessage::user("next"), ChatMessage::assistant("answer")])
        .await
        .unwrap();
    let expected = store.load_messages().await.unwrap();
    drop(store);
    let reopened = SessionManager::new(&dir, Some(&id)).unwrap();
    assert_eq!(reopened.load_messages().await.unwrap(), expected);
}

#[tokio::test]
async fn rejects_orphan_dangling_and_miscorrelated_tools() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    let mut dangling = complete_tool_turn(&["call-1"]);
    dangling.truncate(2);
    assert!(store.append_messages(&id, dangling).await.is_err());

    let orphan = vec![
        complete_tool_turn(&["call-1"])[2].clone(),
        ChatMessage::assistant("done"),
    ];
    assert!(store.append_messages(&id, orphan).await.is_err());

    let mut wrong = complete_tool_turn(&["call-1"]);
    if let ChatMessage::User { content } = &mut wrong[2]
        && let UserContent::ToolResult(result) = &mut content[0]
    {
        result.call = "other".to_string();
    }
    assert!(store.append_messages(&id, wrong).await.is_err());
    assert!(store.load_messages().await.unwrap().is_empty());
}

#[tokio::test]
async fn tool_call_id_reused_across_turns_succeeds() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    store
        .append_messages(&id, complete_tool_turn(&["call-1"]))
        .await
        .unwrap();
    store
        .append_messages(&id, complete_tool_turn(&["call-1"]))
        .await
        .unwrap();
    assert_eq!(store.load_messages().await.unwrap().len(), 8);
}

#[tokio::test]
async fn duplicate_tool_call_id_within_same_message_is_rejected() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    let duplicate = complete_tool_turn(&["call-1", "call-1"]);
    assert!(store.append_messages(&id, duplicate).await.is_err());
}

#[tokio::test]
async fn memory_identity_failures_do_not_change_history() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    assert!(store.ensure_conversation("wrong-id").is_err());
    assert!(
        store
            .append_messages(
                "wrong-id",
                vec![ChatMessage::user("prompt"), ChatMessage::assistant("answer")],
            )
            .await
            .is_err()
    );
    assert!(store.load_messages().await.unwrap().is_empty());
}

#[tokio::test]
async fn clear_preserves_file_and_audit_but_starts_fresh_history() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    store
        .append_messages(&id, vec![ChatMessage::user("old"), ChatMessage::assistant("answer")])
        .await
        .unwrap();
    store
        .append_event(
            SessionEventKind::AssistantResponse,
            serde_json::json!({"status":"complete"}),
        )
        .await
        .unwrap();
    store.clear_messages(&id).await.unwrap();
    assert!(store.load_messages().await.unwrap().is_empty());
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
