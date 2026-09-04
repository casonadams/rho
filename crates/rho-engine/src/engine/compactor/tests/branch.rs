use rho_harness_core::config::Config;
use rig::agent::ModelHandle;
use rig::message::Message;
use rig::test_utils::MockCompletionModel;

use crate::auth::AuthStore;
use crate::engine::AgentEngine;
use crate::engine::builder::AgentEngineBuilder;

async fn test_engine(label: &str, model: Option<MockCompletionModel>) -> AgentEngine {
    let dir = std::env::temp_dir().join(format!("branch_{label}_{}", uuid::Uuid::new_v4()));
    let config = Config {
        sessions_dir: dir.join("sessions"),
        auth_file: dir.join("auth.json"),
        ..Default::default()
    };
    let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
    let mut builder = AgentEngineBuilder::new(config, auth_store)
        .base_dir(dir)
        .tools(Vec::new());
    if let Some(m) = model {
        builder = builder.model(ModelHandle::new(m));
    }
    builder.build().await.unwrap()
}

#[tokio::test]
async fn test_summarize_branch_empty_messages() {
    let engine = test_engine("empty", None).await;
    let summary = engine.summarize_branch(&[]).await;
    assert!(summary.is_empty());
}

#[tokio::test]
async fn test_summarize_branch_fallback_when_no_model() {
    let engine = test_engine("fallback", None).await;

    let messages = vec![
        Message::user("Investigate memory optimization in parser"),
        Message::assistant("Found redundant clone in AST node creation"),
    ];

    let summary = engine.summarize_branch(&messages).await;
    assert!(summary.contains("# Goal"));
    assert!(summary.contains("Investigate memory optimization in parser"));
}

#[tokio::test]
async fn test_summarize_branch_with_llm_model() {
    let mock_response = "# Goal\nOptimize AST memory usage\n\n# Key Decisions\nReplaced clones with Arc";
    let model = MockCompletionModel::text(mock_response);
    let engine = test_engine("llm", Some(model)).await;

    let messages = vec![
        Message::user("Investigate memory optimization"),
        Message::assistant("Done benchmarking"),
    ];

    let summary = engine.summarize_branch(&messages).await;
    assert_eq!(summary, mock_response);
}

#[tokio::test]
async fn test_summarize_branch_redacts_credentials() {
    let secret = "sk-ant-api03-abcdefghijklmnop1234567890abcdefghijklmnop";
    let mock_response = format!("# Critical Context\nDiscovered secret: {secret}");
    let model = MockCompletionModel::text(&mock_response);
    let engine = test_engine("redact", Some(model)).await;
    engine.session_manager.add_secrets(vec![secret.to_string()]).unwrap();

    let messages = vec![
        Message::user("Found an api key in branch"),
        Message::assistant("Logging key"),
    ];

    let summary = engine.summarize_branch(&messages).await;
    assert!(!summary.contains(secret));
    assert!(summary.contains("[REDACTED]"));
}
