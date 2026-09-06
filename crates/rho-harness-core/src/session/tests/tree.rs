use super::{SessionManager, temp_dir};
use rig::message::Message;

#[tokio::test]
async fn test_session_turns_and_rewind() {
    let dir = temp_dir();
    let session = SessionManager::new(&dir, None).unwrap();

    let m1 = Message::user("first prompt");
    let m2 = Message::assistant("first answer");
    session
        .append_messages(&session.session_id, vec![m1, m2])
        .await
        .unwrap();

    let m3 = Message::user("second prompt");
    let m4 = Message::assistant("second answer");
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

    // Rewind back to turn 1
    let retained = session.rewind_to_turn(1).await.unwrap();
    assert_eq!(retained, 2);

    let turns_after = session.load_turns().await.unwrap();
    assert_eq!(turns_after.len(), 1);
    assert_eq!(turns_after[0].user_prompt, "first prompt");
}

async fn setup_root_and_branch_a(session: &SessionManager) -> (String, String) {
    let root = vec![Message::user("Root prompt"), Message::assistant("Root answer")];
    session.append_messages(&session.session_id, root).await.unwrap();
    let root_leaf = session.load_tree().await.unwrap().active_leaf_id.unwrap();

    let branch_a = vec![Message::user("Branch A prompt"), Message::assistant("Branch A answer")];
    session.append_messages(&session.session_id, branch_a).await.unwrap();
    let branch_a_leaf = session.load_tree().await.unwrap().active_leaf_id.unwrap();
    (root_leaf, branch_a_leaf)
}

async fn setup_branch_b(session: &SessionManager, root_leaf: &str) -> String {
    let switched = session.switch_branch(Some(root_leaf.to_string())).await.unwrap();
    assert_eq!(switched[0], Message::user("Root prompt"));
    let branch_b = vec![Message::user("Branch B prompt"), Message::assistant("Branch B answer")];
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
    assert_eq!(resumed_msgs[2], Message::user("Branch B prompt"));
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
    assert_eq!(
        session.get_session_name().await.unwrap(),
        Some("Refactor Auth".to_string())
    );

    session
        .append_messages(
            &session.session_id,
            vec![Message::user("prompt"), Message::assistant("reply")],
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
