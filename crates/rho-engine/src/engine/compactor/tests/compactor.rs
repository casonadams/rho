use rig::agent::ModelHandle;
use rig::message::{AssistantContent, Message, ToolCall, ToolCallId, ToolFunction};
use rig::test_utils::MockCompletionModel;

use crate::engine::compactor::llm::{LlmCompactor, SummarizeOptions};

#[tokio::test]
async fn test_llm_compactor_fallback_when_model_is_none() {
    let compactor = LlmCompactor::new(None);
    let messages = vec![
        Message::user("Please implement feature X in src/app.rs"),
        Message::assistant("I have completed feature X in src/app.rs."),
    ];

    let summary = compactor.summarize(&messages, SummarizeOptions::default()).await;
    assert!(summary.contains("## Goal"));
    assert!(summary.contains("src/app.rs"));
}

#[tokio::test]
async fn test_llm_compactor_successful_llm_call() {
    let mock = MockCompletionModel::text("## Goal\nImplement feature Y\n\n## Progress\n### Done\n- [x] Done");
    let handle = ModelHandle::new(mock.clone());
    let compactor = LlmCompactor::new(Some(handle));

    let messages = vec![
        Message::user("Please implement feature Y"),
        Message::assistant("Done feature Y"),
    ];

    let summary = compactor.summarize(&messages, SummarizeOptions::default()).await;
    assert_eq!(
        summary,
        "## Goal\nImplement feature Y\n\n## Progress\n### Done\n- [x] Done"
    );

    let requests = mock.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].chat_history[0],
        Message::System {
            content: rho_harness_core::session::compaction::SUMMARIZATION_SYSTEM_PROMPT.to_string(),
        }
    );
}

#[tokio::test]
async fn test_llm_compactor_update_with_prior_summary() {
    let mock = MockCompletionModel::text("## Goal\nUpdated goal");
    let handle = ModelHandle::new(mock.clone());
    let compactor = LlmCompactor::new(Some(handle));

    let messages = vec![
        Message::user("Next step for task"),
        Message::assistant("Finished next step"),
    ];

    let prior = "## Goal\nOriginal goal";
    let summary = compactor
        .summarize(
            &messages,
            SummarizeOptions {
                prior_summary: Some(prior),
                custom_instructions: Some("Focus on tests"),
                is_split_turn: false,
            },
        )
        .await;

    assert_eq!(summary, "## Goal\nUpdated goal");
    let requests = mock.requests();
    assert_eq!(requests.len(), 1);
    let prompt_text = format!("{:?}", requests[0]);
    assert!(prompt_text.contains("<previous-summary>"));
    assert!(prompt_text.contains("Focus on tests"));
}

#[tokio::test]
async fn test_llm_compactor_split_turn_summarization() {
    let mock = MockCompletionModel::text("## Early Progress\nPrefix work completed");
    let handle = ModelHandle::new(mock.clone());
    let compactor = LlmCompactor::new(Some(handle));

    let messages = vec![
        Message::user("Do huge operation"),
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall::new(
                ToolCallId::new_or_mint("c1"),
                ToolFunction::new("read".to_string(), serde_json::json!({"path": "src/main.rs"})),
            ))],
        },
    ];

    let summary = compactor
        .summarize(
            &messages,
            SummarizeOptions {
                is_split_turn: true,
                ..Default::default()
            },
        )
        .await;
    assert!(summary.contains("Turn Context (split turn)") || summary.contains("Prefix work completed"));
}
