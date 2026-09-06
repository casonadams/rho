use super::{SessionEventKind, SessionManager, complete_tool_turn, temp_dir};
use rig::memory::ConversationMemory;
use rig::message::Message;

async fn setup_checkpoint(store: &SessionManager, id: &str) -> Vec<Message> {
    let msgs = vec![Message::user("earlier"), Message::assistant("answer")];
    let _ = ConversationMemory::append(store, id, msgs).await;
    let mut checkpoint = complete_tool_turn(&["call-1", "call-2"]);
    checkpoint.pop();
    let _ = store.save_checkpoint(checkpoint.clone()).await;
    checkpoint
}

async fn assert_checkpoint_promoted(store: &SessionManager) {
    assert!(store.load_checkpoint().await.unwrap().is_none());
    assert_eq!(store.load_messages().await.unwrap().len(), 7);
}

async fn promote_checkpoint_in_session(resumed: &SessionManager) {
    let cont = vec![Message::user("please continue"), Message::assistant("done")];
    let _ = resumed.promote_checkpoint(cont).await;
}

async fn promote_and_verify(dir: &std::path::Path, id: &str, checkpoint: Vec<Message>) {
    let resumed = SessionManager::new(dir, Some(id)).unwrap();
    assert_eq!(resumed.load_checkpoint().await.unwrap(), Some(checkpoint));
    promote_checkpoint_in_session(&resumed).await;
    assert_checkpoint_promoted(&resumed).await;
}

#[tokio::test]
async fn budget_checkpoint_resumes_and_promotes_atomically_after_success() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    let checkpoint = setup_checkpoint(&store, &id).await;
    drop(store);
    promote_and_verify(&dir, &id, checkpoint).await;
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

async fn append_cancellation_event(store: &SessionManager, boundary: &str) {
    let payload = serde_json::json!({"boundary": boundary, "terminal": true});
    let _ = store.append_event(SessionEventKind::Cancellation, payload).await;
}

async fn resume_and_append(dir: &std::path::Path, id: &str) {
    let reopened = SessionManager::new(dir, Some(id)).unwrap();
    assert!(reopened.load_messages().await.unwrap().is_empty());
    let msgs = vec![Message::user("after cancel"), Message::assistant("resumed")];
    let _ = ConversationMemory::append(&reopened, id, msgs).await;
}

async fn assert_resumed_count(dir: &std::path::Path, id: &str) {
    let resumed = SessionManager::new(dir, Some(id)).unwrap();
    assert_eq!(resumed.load_messages().await.unwrap().len(), 2);
}

async fn verify_cancellation_boundary(boundary: &str) {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    append_cancellation_event(&store, boundary).await;
    drop(store);
    resume_and_append(&dir, &id).await;
    assert_resumed_count(&dir, &id).await;
}

#[tokio::test]
async fn cancellation_fixtures_remain_parseable_and_resumable() {
    for boundary in [
        "before_first_token",
        "during_text",
        "between_call_result",
        "during_tool",
    ] {
        verify_cancellation_boundary(boundary).await;
    }
}
