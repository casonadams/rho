use super::{SessionManager, temp_dir};
use crate::session::compaction::{CompactionDetails, CompactionMetadata, compaction_summary_message};
use crate::session::tree::{TreeNodeData, TreeNodeKind};
use chrono::Utc;
use rig::message::Message;

#[tokio::test]
async fn single_compaction_projects_summary_and_kept_nodes() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let sid = session.session_id.clone();

    session
        .append_messages(
            &sid,
            vec![Message::user("turn 0 prompt"), Message::assistant("turn 0 reply")],
        )
        .await
        .unwrap();
    session
        .append_messages(
            &sid,
            vec![Message::user("turn 1 prompt"), Message::assistant("turn 1 reply")],
        )
        .await
        .unwrap();
    session
        .append_messages(
            &sid,
            vec![Message::user("turn 2 prompt"), Message::assistant("turn 2 reply")],
        )
        .await
        .unwrap();
    let tree = session.load_tree().await.unwrap();
    let turn2_id = tree.active_leaf_id.clone().unwrap();

    session
        .append_messages(
            &sid,
            vec![Message::user("turn 3 prompt"), Message::assistant("turn 3 reply")],
        )
        .await
        .unwrap();

    let meta = CompactionMetadata {
        summary: "Summary of turns 0 and 1".to_string(),
        first_kept_node_id: Some(turn2_id.clone()),
        tokens_before: 120,
        tokens_after: 50,
        read_files: vec!["src/main.rs".to_string()],
        modified_files: vec!["src/lib.rs".to_string()],
        custom_instructions: Some("keep focus on bug fix".to_string()),
    };

    session
        .append_compaction("Summary of turns 0 and 1", meta)
        .await
        .unwrap();

    let intermediate = session.load_tree().await.unwrap().active_messages();
    assert_eq!(intermediate.len(), 5);
    assert_eq!(intermediate[0], compaction_summary_message("Summary of turns 0 and 1"));
    assert_eq!(intermediate[1], Message::user("turn 2 prompt"));
    assert_eq!(intermediate[2], Message::assistant("turn 2 reply"));
    assert_eq!(intermediate[3], Message::user("turn 3 prompt"));
    assert_eq!(intermediate[4], Message::assistant("turn 3 reply"));

    session
        .append_messages(
            &sid,
            vec![Message::user("turn 4 prompt"), Message::assistant("turn 4 reply")],
        )
        .await
        .unwrap();

    let tree = session.load_tree().await.unwrap();
    let active = tree.active_messages();
    assert_eq!(active.len(), 7);
    assert_eq!(active[0], compaction_summary_message("Summary of turns 0 and 1"));
    assert_eq!(active[1], Message::user("turn 2 prompt"));
    assert_eq!(active[2], Message::assistant("turn 2 reply"));
    assert_eq!(active[3], Message::user("turn 3 prompt"));
    assert_eq!(active[4], Message::assistant("turn 3 reply"));
    assert_eq!(active[5], Message::user("turn 4 prompt"));
    assert_eq!(active[6], Message::assistant("turn 4 reply"));

    let reopened = SessionManager::new(&dir, Some(&sid)).unwrap();
    let reopened_tree = reopened.load_tree().await.unwrap();
    assert_eq!(reopened_tree.active_messages(), active);

    let state = reopened.state.lock().await;
    assert_eq!(state.messages, active);
}

#[tokio::test]
async fn chained_compactions_project_latest_summary_and_kept_nodes() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let sid = session.session_id.clone();

    session
        .append_messages(&sid, vec![Message::user("t0 prompt"), Message::assistant("t0 reply")])
        .await
        .unwrap();
    session
        .append_messages(&sid, vec![Message::user("t1 prompt"), Message::assistant("t1 reply")])
        .await
        .unwrap();
    let tree = session.load_tree().await.unwrap();
    let t1_id = tree.active_leaf_id.clone().unwrap();

    session
        .append_messages(&sid, vec![Message::user("t2 prompt"), Message::assistant("t2 reply")])
        .await
        .unwrap();

    let meta1 = CompactionMetadata {
        summary: "Compaction 1".to_string(),
        first_kept_node_id: Some(t1_id),
        tokens_before: 100,
        tokens_after: 40,
        read_files: vec!["a.rs".to_string()],
        modified_files: Vec::new(),
        custom_instructions: None,
    };
    session.append_compaction("Compaction 1", meta1).await.unwrap();

    session
        .append_messages(&sid, vec![Message::user("t3 prompt"), Message::assistant("t3 reply")])
        .await
        .unwrap();
    let tree = session.load_tree().await.unwrap();
    let t3_id = tree.active_leaf_id.clone().unwrap();

    session
        .append_messages(&sid, vec![Message::user("t4 prompt"), Message::assistant("t4 reply")])
        .await
        .unwrap();

    let meta2 = CompactionMetadata {
        summary: "Compaction 2".to_string(),
        first_kept_node_id: Some(t3_id),
        tokens_before: 150,
        tokens_after: 60,
        read_files: vec!["a.rs".to_string(), "b.rs".to_string()],
        modified_files: vec!["c.rs".to_string()],
        custom_instructions: None,
    };
    session.append_compaction("Compaction 2", meta2).await.unwrap();

    session
        .append_messages(&sid, vec![Message::user("t5 prompt"), Message::assistant("t5 reply")])
        .await
        .unwrap();

    let active = session.load_tree().await.unwrap().active_messages();
    assert_eq!(active.len(), 7);
    assert_eq!(active[0], compaction_summary_message("Compaction 2"));
    assert_eq!(active[1], Message::user("t3 prompt"));
    assert_eq!(active[2], Message::assistant("t3 reply"));
    assert_eq!(active[3], Message::user("t4 prompt"));
    assert_eq!(active[4], Message::assistant("t4 reply"));
    assert_eq!(active[5], Message::user("t5 prompt"));
    assert_eq!(active[6], Message::assistant("t5 reply"));

    let reopened = SessionManager::new(&dir, Some(&sid)).unwrap();
    let state = reopened.state.lock().await;
    assert_eq!(state.messages, active);
}

#[tokio::test]
async fn compaction_branch_isolation() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let sid = session.session_id.clone();

    session
        .append_messages(
            &sid,
            vec![Message::user("root prompt"), Message::assistant("root reply")],
        )
        .await
        .unwrap();
    let root_leaf_id = session.load_tree().await.unwrap().active_leaf_id.clone();

    session
        .append_messages(
            &sid,
            vec![Message::user("branch A prompt"), Message::assistant("branch A reply")],
        )
        .await
        .unwrap();
    let a_turn_id = session.load_tree().await.unwrap().active_leaf_id.clone().unwrap();

    let meta_a = CompactionMetadata {
        summary: "Summary A".to_string(),
        first_kept_node_id: Some(a_turn_id),
        tokens_before: 80,
        tokens_after: 30,
        read_files: Vec::new(),
        modified_files: Vec::new(),
        custom_instructions: None,
    };
    session.append_compaction("Summary A", meta_a).await.unwrap();
    session
        .append_messages(
            &sid,
            vec![Message::user("after compaction A"), Message::assistant("reply A")],
        )
        .await
        .unwrap();
    let branch_a_leaf = session.load_tree().await.unwrap().active_leaf_id.clone();

    session.switch_branch(root_leaf_id).await.unwrap();
    session
        .append_messages(
            &sid,
            vec![
                Message::user("branch B prompt 1"),
                Message::assistant("branch B reply 1"),
            ],
        )
        .await
        .unwrap();
    session
        .append_messages(
            &sid,
            vec![
                Message::user("branch B prompt 2"),
                Message::assistant("branch B reply 2"),
            ],
        )
        .await
        .unwrap();

    let branch_b_msgs = session.load_tree().await.unwrap().active_messages();
    assert_eq!(branch_b_msgs.len(), 6);
    assert_eq!(branch_b_msgs[0], Message::user("root prompt"));
    assert_eq!(branch_b_msgs[1], Message::assistant("root reply"));
    assert_eq!(branch_b_msgs[2], Message::user("branch B prompt 1"));
    assert_eq!(branch_b_msgs[3], Message::assistant("branch B reply 1"));
    assert_eq!(branch_b_msgs[4], Message::user("branch B prompt 2"));
    assert_eq!(branch_b_msgs[5], Message::assistant("branch B reply 2"));

    session.switch_branch(branch_a_leaf).await.unwrap();
    let branch_a_msgs = session.load_tree().await.unwrap().active_messages();
    assert_eq!(branch_a_msgs.len(), 5);
    assert_eq!(branch_a_msgs[0], compaction_summary_message("Summary A"));
    assert_eq!(branch_a_msgs[1], Message::user("branch A prompt"));
    assert_eq!(branch_a_msgs[2], Message::assistant("branch A reply"));
    assert_eq!(branch_a_msgs[3], Message::user("after compaction A"));
    assert_eq!(branch_a_msgs[4], Message::assistant("reply A"));
}

#[tokio::test]
async fn compaction_fallback_when_first_kept_node_id_is_none() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let sid = session.session_id.clone();

    session
        .append_messages(
            &sid,
            vec![Message::user("turn 0 prompt"), Message::assistant("turn 0 reply")],
        )
        .await
        .unwrap();
    session
        .append_messages(
            &sid,
            vec![Message::user("turn 1 prompt"), Message::assistant("turn 1 reply")],
        )
        .await
        .unwrap();

    let meta = CompactionMetadata {
        summary: "Full compaction summary".to_string(),
        first_kept_node_id: None,
        tokens_before: 100,
        tokens_after: 20,
        read_files: Vec::new(),
        modified_files: Vec::new(),
        custom_instructions: None,
    };
    session
        .append_compaction("Full compaction summary", meta)
        .await
        .unwrap();

    let active_at_compaction = session.load_tree().await.unwrap().active_messages();
    assert_eq!(active_at_compaction.len(), 1);
    assert_eq!(
        active_at_compaction[0],
        compaction_summary_message("Full compaction summary")
    );

    session
        .append_messages(
            &sid,
            vec![Message::user("turn 2 prompt"), Message::assistant("turn 2 reply")],
        )
        .await
        .unwrap();

    let active = session.load_tree().await.unwrap().active_messages();
    assert_eq!(active.len(), 3);
    assert_eq!(active[0], compaction_summary_message("Full compaction summary"));
    assert_eq!(active[1], Message::user("turn 2 prompt"));
    assert_eq!(active[2], Message::assistant("turn 2 reply"));
}

#[tokio::test]
async fn compaction_fallback_when_first_kept_node_id_is_missing() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let sid = session.session_id.clone();

    session
        .append_messages(
            &sid,
            vec![Message::user("turn 0 prompt"), Message::assistant("turn 0 reply")],
        )
        .await
        .unwrap();

    let meta = CompactionMetadata {
        summary: "Missing kept node id".to_string(),
        first_kept_node_id: Some("non-existent-node-id".to_string()),
        tokens_before: 80,
        tokens_after: 20,
        read_files: Vec::new(),
        modified_files: Vec::new(),
        custom_instructions: None,
    };
    session.append_compaction("Missing kept node id", meta).await.unwrap();

    session
        .append_messages(
            &sid,
            vec![Message::user("turn 1 prompt"), Message::assistant("turn 1 reply")],
        )
        .await
        .unwrap();

    let active = session.load_tree().await.unwrap().active_messages();
    assert_eq!(active.len(), 3);
    assert_eq!(active[0], compaction_summary_message("Missing kept node id"));
    assert_eq!(active[1], Message::user("turn 1 prompt"));
    assert_eq!(active[2], Message::assistant("turn 1 reply"));
}

#[tokio::test]
async fn compaction_rejects_credentials() {
    let dir = temp_dir();
    let session = SessionManager::new_with_secrets(&dir, None, vec!["credential-token-xyz".to_string()]).unwrap();
    let sid = session.session_id.clone();

    session
        .append_messages(&sid, vec![Message::user("hello"), Message::assistant("hi")])
        .await
        .unwrap();

    let meta_with_secret = CompactionMetadata {
        summary: "Includes credential-token-xyz secret".to_string(),
        first_kept_node_id: None,
        tokens_before: 50,
        tokens_after: 10,
        read_files: Vec::new(),
        modified_files: Vec::new(),
        custom_instructions: None,
    };
    assert!(
        session
            .append_compaction("Includes credential-token-xyz secret", meta_with_secret)
            .await
            .is_err()
    );

    let meta_with_secret_field = CompactionMetadata {
        summary: "Clean summary".to_string(),
        first_kept_node_id: None,
        tokens_before: 50,
        tokens_after: 10,
        read_files: vec!["credential-token-xyz.rs".to_string()],
        modified_files: Vec::new(),
        custom_instructions: None,
    };
    assert!(
        session
            .append_compaction("Clean summary", meta_with_secret_field)
            .await
            .is_err()
    );
}

#[test]
fn compaction_metadata_serialization_roundtrip() {
    let meta = CompactionMetadata {
        summary: "Detailed summary".to_string(),
        first_kept_node_id: Some("node-abc".to_string()),
        tokens_before: 1234,
        tokens_after: 567,
        read_files: vec!["foo.rs".to_string(), "bar.rs".to_string()],
        modified_files: vec!["baz.rs".to_string()],
        custom_instructions: Some("Do not alter logic".to_string()),
    };

    let details = CompactionDetails::from(&meta);
    assert_eq!(details.read_files, vec!["foo.rs", "bar.rs"]);
    assert_eq!(details.modified_files, vec!["baz.rs"]);

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

    let extracted = node.compaction_metadata().unwrap();
    assert_eq!(extracted, meta);

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
