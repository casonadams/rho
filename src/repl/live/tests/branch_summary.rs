use crate::engine::AgentEngine;
use crate::repl::ReplSession;
use crate::repl::live::batch::LiveBatch;
use crate::repl::live::idle::modal_action::{ModalActionContext, apply_modal_key_result};
use crate::repl::live::message::handle_switch_branch;
use crate::repl::live::message::types::BranchSwitchContext;
use crate::repl::live::modal::{ModalKeyResult, handle_modal_key};
use crate::ui::interactive::{ModalKind, NullBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::model::ChatMessage;
use rho_harness_core::session::{SessionManager, TreeNodeKind};

const TITLE: &str = "Summarize Abandoned Branch";

struct Branches {
    session: ReplSession,
    engine: AgentEngine,
    controller: TerminalController<NullBackend>,
    batch: LiveBatch,
    input: crate::repl::input_reader::TerminalInputReader,
    kept: String,
    abandoned: String,
}

async fn answered_turn(manager: &SessionManager, question: &str, answer: &str) {
    manager
        .save_checkpoint(vec![ChatMessage::user(question)])
        .await
        .unwrap();
    manager
        .promote_checkpoint(vec![ChatMessage::assistant(answer)])
        .await
        .unwrap();
}

async fn branches_with_answered_side_branch(dir: &std::path::Path) -> Branches {
    let config = rho_harness_core::config::Config {
        config_dir: dir.to_path_buf(),
        sessions_dir: dir.join("sessions"),
        ..Default::default()
    };
    let session = ReplSession::new(config, crate::auth::AuthStore::default(), None);
    let engine = session.new_engine(None).await.unwrap();
    let manager = &engine.session_manager;
    answered_turn(manager, "kept question", "kept answer").await;
    let kept = manager.active_leaf_id().await.unwrap().unwrap();
    answered_turn(manager, "abandoned question", "abandoned answer").await;
    let abandoned = manager.active_leaf_id().await.unwrap().unwrap();
    Branches {
        session,
        engine,
        controller: crate::repl::test_controller(),
        batch: LiveBatch::new(),
        input: crate::repl::input_reader::TerminalInputReader::spawn_dummy(),
        kept,
        abandoned,
    }
}

async fn request_switch_to_kept(b: &mut Branches) {
    handle_switch_branch(
        BranchSwitchContext {
            session: &mut b.session,
            engine: &mut b.engine,
            controller: &mut b.controller,
        },
        b.kept.clone(),
    )
    .await
    .unwrap();
}

fn press(b: &mut Branches, code: KeyCode) -> ModalKeyResult {
    handle_modal_key(&mut b.controller, KeyEvent::new(code, KeyModifiers::NONE), &mut None).unwrap()
}

async fn apply(b: &mut Branches, result: ModalKeyResult) {
    let ctx = ModalActionContext {
        controller: &mut b.controller,
        session: &mut b.session,
        engine: &mut b.engine,
        input: &mut b.input,
    };
    assert!(apply_modal_key_result(result, ctx, &mut b.batch).await.unwrap());
}

async fn has_branch_summary(b: &Branches) -> bool {
    let tree = b.engine.session_manager.load_tree().await.unwrap();
    tree.nodes.values().any(|n| n.kind == TreeNodeKind::BranchSummary)
}

#[tokio::test]
async fn switching_away_from_an_answered_branch_asks_in_a_modal_before_switching() {
    let temp = rho_test_support::TestDir::new();
    let mut b = branches_with_answered_side_branch(&temp).await;

    request_switch_to_kept(&mut b).await;

    let modal = b.controller.state().active_modal().expect("summary modal is open");
    assert_eq!(modal.title, TITLE);
    assert_eq!(
        modal.kind,
        ModalKind::BranchSummary {
            leaf_id: b.kept.clone(),
            old_leaf: b.abandoned.clone(),
        }
    );
    assert_eq!(
        b.engine.session_manager.active_leaf_id().await.unwrap(),
        Some(b.abandoned.clone())
    );
}

#[tokio::test]
async fn declining_the_summary_switches_without_recording_one() {
    let temp = rho_test_support::TestDir::new();
    let mut b = branches_with_answered_side_branch(&temp).await;
    request_switch_to_kept(&mut b).await;

    press(&mut b, KeyCode::Down);
    let choice = press(&mut b, KeyCode::Enter);
    apply(&mut b, choice).await;

    assert!(b.controller.state().active_modal().is_none());
    assert_eq!(
        b.engine.session_manager.active_leaf_id().await.unwrap(),
        Some(b.kept.clone())
    );
    assert!(!has_branch_summary(&b).await);
}

#[tokio::test]
async fn accepting_the_summary_records_one_and_switches() {
    let temp = rho_test_support::TestDir::new();
    let mut b = branches_with_answered_side_branch(&temp).await;
    request_switch_to_kept(&mut b).await;

    let choice = press(&mut b, KeyCode::Enter);
    apply(&mut b, choice).await;

    let leaf = b.engine.session_manager.active_leaf_id().await.unwrap().unwrap();
    let tree = b.engine.session_manager.load_tree().await.unwrap();
    let summary_node = &tree.nodes[&leaf];
    assert_eq!(summary_node.kind, TreeNodeKind::BranchSummary);
    assert_eq!(summary_node.parent_id, Some(b.kept.clone()));
}

#[tokio::test]
async fn escape_cancels_the_switch_and_keeps_the_current_branch() {
    let temp = rho_test_support::TestDir::new();
    let mut b = branches_with_answered_side_branch(&temp).await;
    request_switch_to_kept(&mut b).await;

    let result = press(&mut b, KeyCode::Esc);

    assert_eq!(result, ModalKeyResult::Handled);
    assert!(b.controller.state().active_modal().is_none());
    assert_eq!(
        b.engine.session_manager.active_leaf_id().await.unwrap(),
        Some(b.abandoned.clone())
    );
}

#[tokio::test]
async fn a_branch_without_an_assistant_turn_switches_without_asking() {
    let temp = rho_test_support::TestDir::new();
    let mut b = branches_with_answered_side_branch(&temp).await;
    b.engine.switch_branch(Some(b.kept.clone())).await.unwrap();
    let session_id = b.engine.session_manager.session_id.clone();
    b.engine
        .session_manager
        .append_messages(&session_id, vec![ChatMessage::user("q"), ChatMessage::assistant("a")])
        .await
        .unwrap();
    b.abandoned = b.engine.session_manager.active_leaf_id().await.unwrap().unwrap();

    request_switch_to_kept(&mut b).await;

    assert!(b.controller.state().active_modal().is_none());
    assert_eq!(
        b.engine.session_manager.active_leaf_id().await.unwrap(),
        Some(b.kept.clone())
    );
}
