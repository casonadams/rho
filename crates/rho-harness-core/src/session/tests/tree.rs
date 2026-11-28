use super::{SessionManager, temp_dir};
use crate::error::Result;
use crate::model::ChatMessage;
use crate::session::RewindOutcome;
use test_case::test_case;

fn note() -> ChatMessage {
    ChatMessage::assistant("stopped")
}

async fn rewind(session: &SessionManager, target_turn: usize) -> Result<RewindOutcome> {
    session.rewind_to_turn(target_turn, note()).await
}

async fn rewind_retained(session: &SessionManager, target_turn: usize) -> usize {
    let outcome = rewind(session, target_turn).await.unwrap();
    assert!(outcome.rewound);
    outcome.retained
}

#[tokio::test]
async fn test_session_turns_and_rewind() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();

    let m1 = ChatMessage::user("first prompt");
    let m2 = ChatMessage::assistant("first answer");
    session
        .append_messages(&session.session_id, vec![m1, m2])
        .await
        .unwrap();

    let m3 = ChatMessage::user("second prompt");
    let m4 = ChatMessage::assistant("second answer");
    session
        .append_messages(&session.session_id, vec![m3, m4])
        .await
        .unwrap();

    let turns = session.load_turns().await.unwrap();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].turn_number, 1);
    assert_eq!(turns[0].user_prompt, "first prompt");
    assert_eq!(turns[1].turn_number, 2);
    assert_eq!(turns[1].user_prompt, "second prompt");

    let retained = rewind_retained(&session, 1).await;
    assert_eq!(retained, 2);

    let turns_after = session.load_turns().await.unwrap();
    assert_eq!(turns_after.len(), 1);
    assert_eq!(turns_after[0].user_prompt, "first prompt");
}

fn turn(n: usize) -> Vec<ChatMessage> {
    vec![
        ChatMessage::user(format!("prompt {n}")),
        ChatMessage::assistant(format!("answer {n}")),
    ]
}

/// Appends each batch as one node and returns the node ids, root first.
async fn append_nodes(session: &SessionManager, batches: &[&[usize]]) -> Vec<String> {
    let mut ids = Vec::new();
    for turns in batches {
        let batch = turns.iter().flat_map(|&n| turn(n)).collect();
        session.append_messages(&session.session_id, batch).await.unwrap();
        ids.push(session.active_leaf_id().await.unwrap().unwrap());
    }
    ids
}

fn log_lines(session: &SessionManager) -> Vec<String> {
    let log = std::fs::read_to_string(&session.file_path).unwrap();
    log.lines().map(str::to_string).collect()
}

async fn assert_reload_matches(session: SessionManager) {
    let (tree, messages) = (
        session.load_tree().await.unwrap(),
        session.load_messages().await.unwrap(),
    );
    let (dir, id) = (
        session.file_path.parent().unwrap().to_path_buf(),
        session.session_id.clone(),
    );
    drop(session);
    let resumed = SessionManager::new(&dir, Some(&id)).unwrap();
    assert_eq!(resumed.load_tree().await.unwrap(), tree);
    assert_eq!(resumed.load_messages().await.unwrap(), messages);
}

#[tokio::test]
async fn rewind_inside_a_node_writes_only_that_nodes_retained_prefix() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let ids = append_nodes(&session, &[&[1], &[2, 3], &[4]]).await;
    let lines_before = log_lines(&session).len();

    let retained = rewind_retained(&session, 2).await;

    assert_eq!(retained, 4);
    let expected: Vec<ChatMessage> = [turn(1), turn(2)].concat();
    assert_eq!(session.load_messages().await.unwrap(), expected);
    let tree = session.load_tree().await.unwrap();
    assert_eq!(tree.len(), 4);
    assert!(ids.iter().all(|id| tree.get_node(id).is_some()));
    let leaf = tree.get_node(tree.active_leaf_id.as_deref().unwrap()).unwrap();
    assert_eq!(leaf.parent_id.as_deref(), Some(ids[0].as_str()));
    assert_eq!(leaf.messages, turn(2));
    let lines = log_lines(&session);
    assert_eq!(lines.len(), lines_before + 1);
    let log = lines.concat();
    for (marker, copies) in [("prompt 1", 1), ("prompt 2", 2), ("prompt 3", 1), ("prompt 4", 1)] {
        assert_eq!(log.matches(marker).count(), copies, "{marker}");
    }
    assert_reload_matches(session).await;
}

#[tokio::test]
async fn rewind_to_a_node_boundary_only_moves_the_active_leaf() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let ids = append_nodes(&session, &[&[1], &[2], &[3]]).await;
    let lines_before = log_lines(&session);

    let retained = rewind_retained(&session, 1).await;

    assert_eq!(retained, 2);
    assert_eq!(session.load_messages().await.unwrap(), turn(1));
    let tree = session.load_tree().await.unwrap();
    assert_eq!(tree.len(), 3);
    assert_eq!(tree.active_leaf_id.as_deref(), Some(ids[0].as_str()));
    let lines = log_lines(&session);
    assert_eq!(lines[..lines_before.len()], lines_before[..]);
    let added: Vec<serde_json::Value> = lines[lines_before.len()..]
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0]["record_type"], "active_leaf_changed");
    assert_eq!(added[0]["active_leaf_id"], ids[0].as_str());
    assert_reload_matches(session).await;
}

#[tokio::test]
async fn rewound_turns_stay_reachable_and_the_listing_counts_every_node() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let ids = append_nodes(&session, &[&[1], &[2, 3]]).await;
    rewind_retained(&session, 1).await;

    let restored = session.switch_branch(Some(ids[1].clone())).await.unwrap();

    assert_eq!(restored, [turn(1), turn(2), turn(3)].concat());
    let summaries = SessionManager::list_session_summaries(&dir).unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].turn_count, session.load_tree().await.unwrap().len());
    assert_eq!(summaries[0].preview, "prompt 1");
}

#[test_case(0; "target zero")]
#[test_case(3; "target past last turn")]
#[test_case(99; "target far past last turn")]
#[tokio::test]
async fn rewind_that_keeps_every_turn_writes_nothing_and_leaves_the_checkpoint_pending(target: usize) {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    append_nodes(&session, &[&[1], &[2]]).await;
    let unfinished = vec![ChatMessage::user("unfinished")];
    session.save_checkpoint(unfinished.clone()).await.unwrap();
    let lines_before = log_lines(&session);

    let outcome = rewind(&session, target).await.unwrap();

    let expected = RewindOutcome {
        retained: 4,
        rewound: false,
    };
    assert_eq!(outcome, expected);
    assert_eq!(log_lines(&session), lines_before);
    assert_eq!(session.load_checkpoint().await.unwrap(), Some(unfinished));
}

#[test_case(&[&[1], &[2]], 1, 2; "rewind into first turn")]
#[test_case(&[&[1], &[2]], 2, 4; "rewind into second turn")]
#[test_case(&[&[1], &[2]], 3, 6; "rewind into pending checkpoint")]
#[test_case(&[], 1, 2; "checkpoint only")]
#[tokio::test]
async fn rewind_with_a_pending_checkpoint_promotes_and_rewinds_in_one_append(
    batches: &[&[usize]],
    target: usize,
    expected: usize,
) {
    let unfinished = vec![
        ChatMessage::user("unfinished 1"),
        ChatMessage::assistant("partial"),
        ChatMessage::user("unfinished 2"),
    ];
    let note = note();
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    append_nodes(&session, batches).await;
    session.save_checkpoint(unfinished.clone()).await.unwrap();
    let history: Vec<ChatMessage> = batches.iter().flat_map(|b| b.iter().flat_map(|&n| turn(n))).collect();
    let lines_before = log_lines(&session).len();

    let retained = rewind_retained(&session, target).await;

    let full: Vec<ChatMessage> = history.iter().chain(&unfinished).chain([&note]).cloned().collect();
    assert_eq!(retained, expected);
    assert_eq!(session.load_messages().await.unwrap(), full[..expected]);
    assert!(session.load_checkpoint().await.unwrap().is_none());
    assert_eq!(log_lines(&session).len(), lines_before + 2);
    let tree = session.load_tree().await.unwrap();
    assert!(
        tree.nodes
            .values()
            .any(|node| node.messages.ends_with(std::slice::from_ref(&note)))
    );
    assert_reload_matches(session).await;
}

fn compaction_metadata(first_kept_node_id: Option<String>) -> crate::session::CompactionMetadata {
    crate::session::CompactionMetadata {
        summary: "Summary".to_string(),
        first_kept_node_id,
        first_kept_message_index: None,
        tokens_before: 10,
        tokens_after: 5,
        read_files: Vec::new(),
        modified_files: Vec::new(),
        custom_instructions: None,
    }
}

#[tokio::test]
async fn rewind_after_a_compaction_keeps_the_summary() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    append_nodes(&session, &[&[1]]).await;
    session
        .append_compaction("Summary", compaction_metadata(None))
        .await
        .unwrap();
    let ids = append_nodes(&session, &[&[2], &[3]]).await;

    let retained = rewind_retained(&session, 1).await;

    assert_eq!(retained, 3);
    let messages = session.load_messages().await.unwrap();
    assert!(matches!(&messages[0], ChatMessage::System { content } if content == "Summary"));
    assert_eq!(messages[1..], turn(2)[..]);
    assert_eq!(
        session.active_leaf_id().await.unwrap().as_deref(),
        Some(ids[0].as_str())
    );
    assert_reload_matches(session).await;
}

#[tokio::test]
async fn rewind_into_history_kept_by_a_compaction_is_refused() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let kept = append_nodes(&session, &[&[1], &[2]]).await;
    session
        .append_compaction("Summary", compaction_metadata(Some(kept[0].clone())))
        .await
        .unwrap();
    append_nodes(&session, &[&[3]]).await;
    let before = session.load_messages().await.unwrap();
    let lines_before = log_lines(&session);

    let error = rewind(&session, 1).await.unwrap_err().to_string();

    assert!(error.contains("before the last compaction"), "{error}");
    assert_eq!(session.load_messages().await.unwrap(), before);
    assert_eq!(log_lines(&session), lines_before);
}

async fn setup_root_and_branch_a(session: &SessionManager) -> (String, String) {
    let root = vec![ChatMessage::user("Root prompt"), ChatMessage::assistant("Root answer")];
    session.append_messages(&session.session_id, root).await.unwrap();
    let root_leaf = session.load_tree().await.unwrap().active_leaf_id.unwrap();

    let branch_a = vec![
        ChatMessage::user("Branch A prompt"),
        ChatMessage::assistant("Branch A answer"),
    ];
    session.append_messages(&session.session_id, branch_a).await.unwrap();
    let branch_a_leaf = session.load_tree().await.unwrap().active_leaf_id.unwrap();
    (root_leaf, branch_a_leaf)
}

async fn setup_branch_b(session: &SessionManager, root_leaf: &str) -> String {
    let switched = session.switch_branch(Some(root_leaf.to_string())).await.unwrap();
    assert_eq!(switched[0], ChatMessage::user("Root prompt"));
    let branch_b = vec![
        ChatMessage::user("Branch B prompt"),
        ChatMessage::assistant("Branch B answer"),
    ];
    session.append_messages(&session.session_id, branch_b).await.unwrap();
    session.load_tree().await.unwrap().active_leaf_id.unwrap()
}

async fn assert_resumed_tree(resumed: &SessionManager, branch_b_leaf: &str) {
    let resumed_tree = resumed.load_tree().await.unwrap();
    assert_eq!(resumed_tree.len(), 3);
    assert_eq!(resumed_tree.active_leaf_id.as_deref(), Some(branch_b_leaf));
}

async fn assert_resumed_msgs(resumed: &SessionManager) {
    let resumed_msgs = resumed.load_messages().await.unwrap();
    assert_eq!(resumed_msgs.len(), 4);
    assert_eq!(resumed_msgs[2], ChatMessage::user("Branch B prompt"));
}

#[tokio::test]
async fn test_session_tree_dag_branching_and_ancestors() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let (root_leaf, branch_a_leaf) = setup_root_and_branch_a(&session).await;
    let branch_b_leaf = setup_branch_b(&session, &root_leaf).await;

    let tree_b = session.load_tree().await.unwrap();
    let (unique_a, unique_b) = tree_b.branch_divergence(&branch_a_leaf, &branch_b_leaf);
    assert_eq!(unique_a.len(), 1);
    assert_eq!(unique_b.len(), 1);

    let resumed = SessionManager::new(&dir, Some(&session.session_id)).unwrap();
    assert_resumed_tree(&resumed, &branch_b_leaf).await;
    assert_resumed_msgs(&resumed).await;
}

#[tokio::test]
async fn test_session_tree_node_label_and_naming() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();

    session.set_session_name("Refactor Auth").await.unwrap();
    assert_eq!(session.cached_session_name(), Some("Refactor Auth".to_string()));

    session
        .append_messages(
            &session.session_id,
            vec![ChatMessage::user("prompt"), ChatMessage::assistant("reply")],
        )
        .await
        .unwrap();

    let leaf_id = session.active_leaf_id().await.unwrap().unwrap();
    session
        .set_node_label(&leaf_id, Some("milestone-1".to_string()))
        .await
        .unwrap();

    let tree = session.load_tree().await.unwrap();
    let node = tree.get_node(&leaf_id).unwrap();
    assert_eq!(node.label, Some("milestone-1".to_string()));
}

#[tokio::test]
async fn test_auto_session_name_respects_user_provenance() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();

    assert!(session.set_auto_session_name("Auto Title").await.unwrap());
    assert_eq!(session.cached_session_name().as_deref(), Some("Auto Title"));
    assert_eq!(
        session.cached_session_name_source(),
        Some(crate::session::TitleSource::Auto)
    );

    session.set_session_name("Manual Title").await.unwrap();
    assert_eq!(session.cached_session_name().as_deref(), Some("Manual Title"));
    assert_eq!(
        session.cached_session_name_source(),
        Some(crate::session::TitleSource::User)
    );

    assert!(!session.set_auto_session_name("Late Auto Title").await.unwrap());
    assert_eq!(session.cached_session_name().as_deref(), Some("Manual Title"));

    let session_id = session.session_id.clone();
    drop(session);

    let resumed = SessionManager::new(&dir, Some(&session_id)).unwrap();
    assert_eq!(resumed.cached_session_name().as_deref(), Some("Manual Title"));
    assert_eq!(
        resumed.cached_session_name_source(),
        Some(crate::session::TitleSource::User)
    );
    assert!(!resumed.set_auto_session_name("After Resume Auto").await.unwrap());
}

#[tokio::test]
async fn switching_to_an_unknown_node_is_rejected_and_changes_nothing() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();
    let (_, branch_a_leaf) = setup_root_and_branch_a(&session).await;
    let before = session.load_messages().await.unwrap();
    let size_before = std::fs::metadata(&session.file_path).unwrap().len();

    let err = session
        .switch_branch(Some("no-such-node".to_string()))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("no-such-node"), "{err}");
    assert_eq!(
        session.active_leaf_id().await.unwrap().as_deref(),
        Some(branch_a_leaf.as_str())
    );
    assert_eq!(session.load_messages().await.unwrap(), before);
    assert_eq!(std::fs::metadata(&session.file_path).unwrap().len(), size_before);
    assert!(session.switch_branch(None).await.is_ok());
}
