use super::{SessionManager, temp_dir};
use crate::session::compaction::{CompactionDetails, CompactionMetadata, compaction_summary_message};
use crate::session::tree::{TreeNodeData, TreeNodeKind};
use chrono::Utc;
use rig::message::Message;

async fn append_turn(session: &SessionManager, sid: &str, n: usize) -> Option<String> {
    let prompt = format!("turn {n} prompt");
    let reply = format!("turn {n} reply");
    session
        .append_messages(sid, vec![Message::user(prompt), Message::assistant(reply)])
        .await
        .unwrap();
    session.load_tree().await.unwrap().active_leaf_id
}

fn sample_metadata(summary: &str, kept: Option<String>) -> CompactionMetadata {
    CompactionMetadata {
        summary: summary.to_string(),
        first_kept_node_id: kept,
        tokens_before: 120,
        tokens_after: 50,
        read_files: vec!["src/main.rs".to_string()],
        modified_files: vec!["src/lib.rs".to_string()],
        custom_instructions: Some("keep focus on bug fix".to_string()),
    }
}

async fn setup_single_compaction_tree(session: &SessionManager, sid: &str) -> String {
    for n in 0..2 {
        append_turn(session, sid, n).await;
    }
    let turn2_id = append_turn(session, sid, 2).await.unwrap();
    append_turn(session, sid, 3).await;
    turn2_id
}

#[tokio::test]
async fn single_compaction_projects_summary_and_kept_nodes() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let sid = session.session_id.clone();
    let turn2_id = setup_single_compaction_tree(&session, &sid).await;

    let meta = sample_metadata("Summary of turns 0 and 1", Some(turn2_id));
    session
        .append_compaction("Summary of turns 0 and 1", meta)
        .await
        .unwrap();

    let intermediate = session.load_tree().await.unwrap().active_messages();
    assert_eq!(intermediate.len(), 5);
    assert_eq!(intermediate[0], compaction_summary_message("Summary of turns 0 and 1"));
    assert_eq!(intermediate[1], Message::user("turn 2 prompt"));

    append_turn(&session, &sid, 4).await;
    let active = session.load_tree().await.unwrap().active_messages();
    assert_eq!(active.len(), 7);
    assert_eq!(active[0], compaction_summary_message("Summary of turns 0 and 1"));
    assert_eq!(active[5], Message::user("turn 4 prompt"));

    let reopened = SessionManager::new(&dir, Some(&sid)).unwrap();
    assert_eq!(reopened.load_tree().await.unwrap().active_messages(), active);
}

async fn append_first_compaction(session: &SessionManager, sid: &str) {
    append_turn(session, sid, 0).await;
    let t1_id = append_turn(session, sid, 1).await;
    append_turn(session, sid, 2).await;
    session
        .append_compaction("Compaction 1", sample_metadata("Compaction 1", t1_id))
        .await
        .unwrap();
}

async fn append_second_compaction(session: &SessionManager, sid: &str) {
    let t3_id = append_turn(session, sid, 3).await;
    append_turn(session, sid, 4).await;
    session
        .append_compaction("Compaction 2", sample_metadata("Compaction 2", t3_id))
        .await
        .unwrap();
    append_turn(session, sid, 5).await;
}

async fn setup_chained_compactions(session: &SessionManager, sid: &str) {
    append_first_compaction(session, sid).await;
    append_second_compaction(session, sid).await;
}

#[tokio::test]
async fn chained_compactions_project_latest_summary_and_kept_nodes() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let sid = session.session_id.clone();
    setup_chained_compactions(&session, &sid).await;

    let active = session.load_tree().await.unwrap().active_messages();
    assert_eq!(active.len(), 7);
    assert_eq!(active[0], compaction_summary_message("Compaction 2"));
    assert_eq!(active[1], Message::user("turn 3 prompt"));
    assert_eq!(active[5], Message::user("turn 5 prompt"));

    let reopened = SessionManager::new(&dir, Some(&sid)).unwrap();
    assert_eq!(reopened.state.lock().await.messages, active);
}

async fn append_branch_a_compaction(session: &SessionManager, sid: &str, a_turn_id: Option<String>) {
    let meta = sample_metadata("Summary A", a_turn_id);
    session.append_compaction("Summary A", meta).await.unwrap();
    append_turn(session, sid, 2).await;
}

async fn setup_compaction_branch_a(session: &SessionManager, sid: &str) -> (Option<String>, Option<String>) {
    let root_id = append_turn(session, sid, 0).await;
    let a_turn_id = append_turn(session, sid, 1).await;
    append_branch_a_compaction(session, sid, a_turn_id).await;
    let branch_a_leaf = session.load_tree().await.unwrap().active_leaf_id;
    (root_id, branch_a_leaf)
}

#[tokio::test]
async fn compaction_branch_isolation() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let sid = session.session_id.clone();
    let (root_leaf_id, branch_a_leaf) = setup_compaction_branch_a(&session, &sid).await;

    session.switch_branch(root_leaf_id).await.unwrap();
    append_turn(&session, &sid, 10).await;
    append_turn(&session, &sid, 11).await;
    assert_eq!(session.load_tree().await.unwrap().active_messages().len(), 6);

    session.switch_branch(branch_a_leaf).await.unwrap();
    let branch_a_msgs = session.load_tree().await.unwrap().active_messages();
    assert_eq!(branch_a_msgs.len(), 5);
    assert_eq!(branch_a_msgs[0], compaction_summary_message("Summary A"));
}

#[tokio::test]
async fn compaction_fallback_when_first_kept_node_id_is_none() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let sid = session.session_id.clone();
    append_turn(&session, &sid, 0).await;
    append_turn(&session, &sid, 1).await;

    let meta = sample_metadata("Full compaction summary", None);
    session
        .append_compaction("Full compaction summary", meta)
        .await
        .unwrap();
    assert_eq!(session.load_tree().await.unwrap().active_messages().len(), 1);

    append_turn(&session, &sid, 2).await;
    let active = session.load_tree().await.unwrap().active_messages();
    assert_eq!(active.len(), 3);
    assert_eq!(active[0], compaction_summary_message("Full compaction summary"));
}

#[tokio::test]
async fn compaction_fallback_when_first_kept_node_id_is_missing() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let sid = session.session_id.clone();
    append_turn(&session, &sid, 0).await;

    let meta = sample_metadata("Missing kept node id", Some("non-existent-node-id".to_string()));
    session.append_compaction("Missing kept node id", meta).await.unwrap();

    append_turn(&session, &sid, 1).await;
    let active = session.load_tree().await.unwrap().active_messages();
    assert_eq!(active.len(), 3);
    assert_eq!(active[0], compaction_summary_message("Missing kept node id"));
}

#[tokio::test]
async fn compaction_rejects_credentials() {
    let dir = temp_dir();
    let session = SessionManager::new_with_secrets(&dir, None, vec!["credential-token-xyz".to_string()]).unwrap();
    let sid = session.session_id.clone();
    append_turn(&session, &sid, 0).await;

    let secret_summary = sample_metadata("Includes credential-token-xyz secret", None);
    assert!(session.append_compaction("fail1", secret_summary).await.is_err());

    let mut secret_field = sample_metadata("Clean summary", None);
    secret_field.read_files = vec!["credential-token-xyz.rs".to_string()];
    assert!(session.append_compaction("fail2", secret_field).await.is_err());
}

fn sample_roundtrip_metadata() -> CompactionMetadata {
    CompactionMetadata {
        summary: "Detailed summary".to_string(),
        first_kept_node_id: Some("node-abc".to_string()),
        tokens_before: 1234,
        tokens_after: 567,
        read_files: vec!["foo.rs".to_string(), "bar.rs".to_string()],
        modified_files: vec!["baz.rs".to_string()],
        custom_instructions: Some("Do not alter logic".to_string()),
    }
}

#[test]
fn compaction_metadata_details_and_serialization() {
    let meta = sample_roundtrip_metadata();
    let details = CompactionDetails::from(&meta);
    assert_eq!(details.read_files, ["foo.rs", "bar.rs"]);
    assert_eq!(details.modified_files, ["baz.rs"]);

    let json = serde_json::to_value(&meta).unwrap();
    let node = TreeNodeData {
        id: "compaction-node".to_string(),
        parent_id: None,
        timestamp: Utc::now(),
        kind: TreeNodeKind::Compaction,
        messages: vec![compaction_summary_message("Detailed summary")],
        label: Some("Compaction".to_string()),
        metadata: Some(json),
    };
    assert_eq!(node.compaction_metadata().unwrap(), meta);
}

#[test]
fn compaction_metadata_non_compaction_node() {
    let non_compaction_node = TreeNodeData {
        id: "turn-node".to_string(),
        parent_id: None,
        timestamp: Utc::now(),
        kind: TreeNodeKind::UserTurn,
        messages: Vec::new(),
        label: None,
        metadata: None,
    };
    assert!(non_compaction_node.compaction_metadata().is_none());
}
