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

#[tokio::test]
async fn compact_context_executes_and_prints_token_savings() {
    let temp = std::env::temp_dir().join(format!("compact_cmd_{}", uuid::Uuid::new_v4()));
    let config = Config {
        sessions_dir: temp.join("sessions"),
        keep_recent_tokens: 10,
        ..Config::default()
    };
    std::fs::create_dir_all(&config.sessions_dir).unwrap();
    let session_mgr = SessionManager::new(&config.sessions_dir, None).unwrap();
    let sid = session_mgr.session_id.clone();

    for i in 0..4 {
        let u = Message::user(format!(
            "Detailed query {i} with long description to consume context tokens"
        ));
        let a = Message::assistant(format!(
            "Comprehensive answer {i} analyzing the system and reviewing code"
        ));
        session_mgr.append(&sid, vec![u, a]).await.unwrap();
    }

    let mock_response = "## Goal\nAnalyze queries\n\n## Progress\nCompleted analyses";
    let engine = mock_engine_with_session(
        MockCompletionModel::text(mock_response),
        MockEngineConfig {
            base_dir: &temp,
            app_config: config.clone(),
            session_manager: Some(session_mgr),
            built_in_tools: None,
        },
    );

    let (renderer, mut events) = collecting_renderer();
    let mut session = ReplSession::new(config, AuthStore::default(), None);
    session.renderer = renderer;

    compact_context(&session, &engine, Some("preserve key decisions")).await;

    let output = collected_output(&mut events);
    assert!(output.contains("[Compacting conversation context...]"));
    assert!(output.contains("[Compacted context:"));
    assert!(output.contains("->"));
    assert!(output.contains("tokens (saved"));

    let _ = std::fs::remove_dir_all(temp);
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

    let root_msgs = vec![Message::user("Root prompt"), Message::assistant("Root initial reply")];
    session_mgr.append(&sid, root_msgs).await.unwrap();
    let root_leaf = session_mgr.active_leaf_id().await.unwrap().unwrap();

    let branch_msgs = vec![
        Message::user("Branch user question"),
        Message::assistant("Explored alternative algorithm"),
    ];
    session_mgr.append(&sid, branch_msgs).await.unwrap();
    let branch_leaf = session_mgr.active_leaf_id().await.unwrap().unwrap();

    let mock_response = "# Goal\nExplore alternative algorithm\n\n# Key Decisions\nFound O(n) approach";
    let engine = mock_engine_with_session(
        MockCompletionModel::text(mock_response),
        MockEngineConfig {
            base_dir: &temp,
            app_config: config.clone(),
            session_manager: Some(session_mgr.clone()),
            built_in_tools: None,
        },
    );

    let abandoned_messages = vec![Message::assistant("Explored alternative algorithm")];
    let summary = engine.summarize_branch(&abandoned_messages).await;
    assert_eq!(summary, mock_response);

    session_mgr.switch_branch(Some(root_leaf)).await.unwrap();
    session_mgr.append_branch_summary(&summary, &branch_leaf).await.unwrap();

    let tree = session_mgr.load_tree().await.unwrap();
    let active = tree.active_messages();
    assert_eq!(active.len(), 3);
    let summary_msg = &active[2];
    let serialized = format!("{summary_msg:?}");
    assert!(serialized.contains(&format!("[Branch Summary from {branch_leaf}]")));
    assert!(serialized.contains("Found O(n) approach"));

    let _ = std::fs::remove_dir_all(temp);
}
