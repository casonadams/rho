use super::super::tree::TreeNodeKind;
use super::{
    LEGACY_V2_ID, LEGACY_V2_SESSION, SessionEventKind, SessionManager, complete_tool_turn, legacy_v2_turns, temp_dir,
    tool_round,
};
use crate::model::ChatMessage;

/// Runs one turn the way the engine does: two checkpointed tool rounds, then the final answer.
async fn promoted_turn(store: &SessionManager, n: usize) -> Vec<ChatMessage> {
    let first = tool_round(&format!("prompt-{n}"), &[&format!("call-{n}a")]);
    let second = tool_round("unused", &[&format!("call-{n}b")]).split_off(1);
    let answer = ChatMessage::assistant(format!("answer-{n}"));
    store.extend_checkpoint(first.clone()).await.unwrap();
    store.extend_checkpoint(second.clone()).await.unwrap();
    store.promote_checkpoint(vec![answer.clone()]).await.unwrap();
    first.into_iter().chain(second).chain([answer]).collect()
}

#[tokio::test]
async fn promoted_turns_reload_to_the_same_tree() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    let mut turns = Vec::new();
    for n in 1..=3 {
        turns.push(promoted_turn(&store, n).await);
    }
    let live_tree = store.state.lock().await.tree.clone();
    drop(store);

    let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
    let tree = resumed.state.lock().await.tree.clone();
    assert_eq!(tree, live_tree);
    let path = tree.ancestor_nodes(tree.active_leaf_id.as_deref().unwrap());
    let nodes: Vec<_> = path
        .iter()
        .map(|node| (node.kind.clone(), node.messages.clone()))
        .collect();
    let expected: Vec<_> = turns
        .iter()
        .map(|turn| (TreeNodeKind::AssistantTurn, turn.clone()))
        .collect();
    assert_eq!(nodes, expected);
    assert_eq!(tree.len(), 3);
    assert_eq!(resumed.load_messages().await.unwrap(), turns.concat());
    assert!(resumed.load_checkpoint().await.unwrap().is_none());
}

#[tokio::test]
async fn promoted_turns_write_each_message_once() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    for n in 1..=3 {
        promoted_turn(&store, n).await;
    }
    let log = std::fs::read_to_string(&store.file_path).unwrap();
    assert!(log.starts_with(r#"{"record_type":"header","version":3,"#), "{log}");
    for n in 1..=3 {
        for marker in [format!("\"prompt-{n}\""), format!("\"answer-{n}\"")] {
            assert_eq!(log.matches(&marker).count(), 1, "{marker}");
        }
    }
}

#[tokio::test]
async fn version_2_session_loads_and_keeps_its_format() {
    let dir = temp_dir();
    let path = dir.join(format!("{LEGACY_V2_ID}.jsonl"));
    std::fs::write(&path, LEGACY_V2_SESSION).unwrap();

    let resumed = SessionManager::new(&dir, Some(LEGACY_V2_ID)).unwrap();
    let [first, second] = legacy_v2_turns();
    assert_eq!(
        resumed.load_messages().await.unwrap(),
        [first.clone(), second.clone()].concat()
    );
    let tree = resumed.state.lock().await.tree.clone();
    assert_eq!(tree.get_node("node-1").unwrap().messages, first);
    assert_eq!(tree.get_node("node-2").unwrap().parent_id.as_deref(), Some("node-1"));
    assert_eq!(tree.active_leaf_id.as_deref(), Some("node-2"));

    let third = promoted_turn(&resumed, 3).await;
    drop(resumed);
    let log = std::fs::read_to_string(&path).unwrap();
    assert!(log.starts_with(LEGACY_V2_SESSION));
    let appended: Vec<serde_json::Value> = log[LEGACY_V2_SESSION.len()..]
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(appended.last().unwrap()["record_type"], "tree_node");
    let reloaded = SessionManager::new(&dir, Some(LEGACY_V2_ID)).unwrap();
    assert_eq!(reloaded.load_messages().await.unwrap(), [first, second, third].concat());
}

async fn setup_checkpoint(store: &SessionManager, id: &str) -> Vec<ChatMessage> {
    let msgs = vec![ChatMessage::user("earlier"), ChatMessage::assistant("answer")];
    let _ = store.append_messages(id, msgs).await;
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
    let cont = vec![ChatMessage::user("please continue"), ChatMessage::assistant("done")];
    let _ = resumed.promote_checkpoint(cont).await;
}

async fn promote_and_verify(dir: &std::path::Path, id: &str, checkpoint: Vec<ChatMessage>) {
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
        .save_checkpoint(vec![ChatMessage::user("credential-sentinel")])
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
async fn extended_checkpoints_append_batches_and_survive_reload() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let id = store.session_id.clone();
    let mut first = complete_tool_turn(&["call-1"]);
    first.pop();
    let mut second = complete_tool_turn(&["call-2"]);
    second.pop();
    let second = second.split_off(1);

    store.extend_checkpoint(first.clone()).await.unwrap();
    store.extend_checkpoint(second.clone()).await.unwrap();
    let log = std::fs::read_to_string(&store.file_path).unwrap();
    assert_eq!(
        log.lines().filter(|line| line.contains("call-1")).count(),
        1,
        "earlier batches must not be rewritten"
    );
    drop(store);

    let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
    let expected: Vec<ChatMessage> = first.into_iter().chain(second).collect();
    assert_eq!(resumed.load_checkpoint().await.unwrap(), Some(expected));
    resumed
        .promote_checkpoint(vec![ChatMessage::assistant("done")])
        .await
        .unwrap();
    assert!(resumed.load_checkpoint().await.unwrap().is_none());
    assert_eq!(resumed.load_messages().await.unwrap().len(), 6);
}

#[tokio::test]
async fn checkpoint_extensions_reject_dangling_tool_calls() {
    let dir = temp_dir();
    let store = SessionManager::new(&dir, None).unwrap();
    let mut dangling = complete_tool_turn(&["call-1"]);
    dangling.truncate(2);
    assert!(store.extend_checkpoint(dangling).await.is_err());
    assert!(store.extend_checkpoint(Vec::new()).await.is_err());
    assert!(store.load_checkpoint().await.unwrap().is_none());
}

async fn append_cancellation_event(store: &SessionManager, boundary: &str) {
    let payload = serde_json::json!({"boundary": boundary, "terminal": true});
    let _ = store.append_event(SessionEventKind::Cancellation, payload).await;
}

async fn resume_and_append(dir: &std::path::Path, id: &str) {
    let reopened = SessionManager::new(dir, Some(id)).unwrap();
    assert!(reopened.load_messages().await.unwrap().is_empty());
    let msgs = vec![ChatMessage::user("after cancel"), ChatMessage::assistant("resumed")];
    let _ = reopened.append_messages(id, msgs).await;
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
