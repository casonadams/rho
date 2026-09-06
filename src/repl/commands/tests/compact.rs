use super::{collected_output, collecting_renderer, test_context};
use crate::config::Config;
use crate::repl::ReplSession;
use crate::repl::commands::{CommandResult, SlashCommandHandler};
use crate::repl::line_mode::dispatch::compact_context;
use rho_engine::auth::AuthStore;
use rho_engine::engine::eval::mock::{MockEngineConfig, mock_engine_with_session};
use rho_harness_core::session::SessionManager;
use rig::memory::ConversationMemory;
use rig::message::Message;
use rig::test_utils::MockCompletionModel;

#[tokio::test]
async fn compact_command_without_instructions_dispatches() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/compact", &mut context).await.unwrap();

    assert_eq!(result, Some(CommandResult::Compact { instructions: None }));
}

#[tokio::test]
async fn compact_command_with_instructions_dispatches() {
    let mut config = Config::default();
    let mut auth = AuthStore::default();
    let (renderer, _) = collecting_renderer();
    let mut context = test_context(&mut config, &mut auth, &renderer);

    let result = SlashCommandHandler::handle("/compact focus on errors and tests", &mut context)
        .await
        .unwrap();

    assert_eq!(
        result,
        Some(CommandResult::Compact {
            instructions: Some("focus on errors and tests".to_string())
        })
    );
}

async fn seed_long_turns(session_mgr: &SessionManager, sid: &str) {
    for i in 0..4 {
        let u = Message::user(format!(
            "Detailed query {i} with long description to consume context tokens"
        ));
        let a = Message::assistant(format!(
            "Comprehensive answer {i} analyzing the system and reviewing code"
        ));
        session_mgr.append(sid, vec![u, a]).await.unwrap();
    }
}

fn setup_compact_env(
    temp: &std::path::Path,
) -> (
    ReplSession,
    crate::engine::AgentEngine,
    tokio::sync::mpsc::UnboundedReceiver<crate::ui::interactive::UiEvent>,
) {
    let config = Config {
        sessions_dir: temp.join("sessions"),
        keep_recent_tokens: 10,
        ..Config::default()
    };
    std::fs::create_dir_all(&config.sessions_dir).unwrap();
    let sm = SessionManager::new(&config.sessions_dir, None).unwrap();
    let engine = mock_engine_with_session(
        MockCompletionModel::text("## Goal\nAnalyze queries\n\n## Progress\nCompleted analyses"),
        MockEngineConfig {
            base_dir: temp,
            app_config: config.clone(),
            session_manager: Some(sm),
            built_in_tools: None,
        },
    );
    let (renderer, events) = collecting_renderer();
    let mut session = ReplSession::new(config, AuthStore::default(), None);
    session.renderer = renderer;
    (session, engine, events)
}

#[tokio::test]
async fn compact_context_executes_and_prints_token_savings() {
    let temp = std::env::temp_dir().join(format!("compact_cmd_{}", uuid::Uuid::new_v4()));
    let (session, engine, mut events) = setup_compact_env(&temp);
    seed_long_turns(&engine.session_manager, &engine.session_manager.session_id).await;

    compact_context(&session, &engine, Some("preserve key decisions")).await;
    let output = collected_output(&mut events);
    assert!(output.contains("[Compacting conversation context...]") && output.contains("[Compacted context:"));
    let _ = std::fs::remove_dir_all(temp);
}

async fn setup_branching_session(sm: &SessionManager, sid: &str) -> (String, String) {
    sm.append(
        sid,
        vec![Message::user("Root prompt"), Message::assistant("Root initial reply")],
    )
    .await
    .unwrap();
    let root_leaf = sm.active_leaf_id().await.unwrap().unwrap();
    sm.append(
        sid,
        vec![
            Message::user("Branch question"),
            Message::assistant("Explored alternative algorithm"),
        ],
    )
    .await
    .unwrap();
    let branch_leaf = sm.active_leaf_id().await.unwrap().unwrap();
    (root_leaf, branch_leaf)
}

async fn assert_branch_summary_recorded(sm: &SessionManager, (root, branch, summary): (&str, &str, &str)) {
    sm.switch_branch(Some(root.to_string())).await.unwrap();
    sm.append_branch_summary(summary, branch).await.unwrap();
    let tree = sm.load_tree().await.unwrap();
    let serialized = format!("{:?}", tree.active_messages()[2]);
    assert!(
        serialized.contains(&format!("[Branch Summary from {branch}]")) && serialized.contains("Found O(n) approach")
    );
}

#[tokio::test]
async fn branch_summarization_records_structured_summary() {
    let temp = std::env::temp_dir().join(format!("branch_sum_{}", uuid::Uuid::new_v4()));
    let config = Config {
        sessions_dir: temp.join("sessions"),
        ..Config::default()
    };
    std::fs::create_dir_all(&config.sessions_dir).unwrap();
    let session_mgr = SessionManager::new(&config.sessions_dir, None).unwrap();
    let sid = session_mgr.session_id.clone();
    let (root_leaf, branch_leaf) = setup_branching_session(&session_mgr, &sid).await;

    let mock_response = "# Goal\nExplore alternative algorithm\n\n# Key Decisions\nFound O(n) approach";
    let engine = mock_engine_with_session(
        MockCompletionModel::text(mock_response),
        MockEngineConfig {
            base_dir: &temp,
            app_config: config,
            session_manager: Some(session_mgr.clone()),
            built_in_tools: None,
        },
    );

    let summary = engine
        .summarize_branch(&[Message::assistant("Explored alternative algorithm")])
        .await;
    assert_eq!(summary, mock_response);
    assert_branch_summary_recorded(&session_mgr, (&root_leaf, &branch_leaf, &summary)).await;
    let _ = std::fs::remove_dir_all(temp);
}
