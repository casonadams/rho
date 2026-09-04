use super::{SessionEventKind, SessionManager, complete_tool_turn, temp_dir};
use rig::memory::ConversationMemory;
use rig::message::Message;

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
