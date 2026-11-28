//! Compactor tests: LLM summarization, orchestration, overflow recovery,
//! proactive auto-compaction, and sequential compaction accumulation.

mod branch {
    use super::common::test_engine;
    use rho_harness_core::model::ChatMessage;
    use std::sync::Arc;

    use crate::provider::adapter::MockAdapter;

    #[tokio::test]
    async fn test_summarize_branch_empty_messages() {
        let (engine, _dir) = test_engine("empty", None, None).await;
        let summary = engine.summarize_branch(&[]).await;
        assert!(summary.is_empty());
    }

    #[tokio::test]
    async fn test_summarize_branch_fallback_when_no_model() {
        let (engine, _dir) = test_engine("fallback", None, None).await;

        let messages = vec![
            ChatMessage::user("Investigate memory optimization in parser"),
            ChatMessage::assistant("Found redundant clone in AST node creation"),
        ];

        let summary = engine.summarize_branch(&messages).await;
        assert!(summary.contains("# Goal"));
        assert!(summary.contains("Investigate memory optimization in parser"));
    }

    #[tokio::test]
    async fn test_summarize_branch_with_llm_model() {
        let mock_response = "# Goal\nOptimize AST memory usage\n\n# Key Decisions\nReplaced clones with Arc";
        let model = Arc::new(MockAdapter::text(mock_response));
        let (engine, _dir) = test_engine("llm", None, Some(model)).await;

        let messages = vec![
            ChatMessage::user("Investigate memory optimization"),
            ChatMessage::assistant("Done benchmarking"),
        ];

        let summary = engine.summarize_branch(&messages).await;
        assert_eq!(summary, mock_response);
    }

    #[tokio::test]
    async fn test_summarize_branch_redacts_credentials() {
        let secret = "sk-ant-api03-abcdefghijklmnop1234567890abcdefghijklmnop";
        let mock_response = format!("# Critical Context\nDiscovered secret: {secret}");
        let model = Arc::new(MockAdapter::text(&mock_response));
        let (engine, _dir) = test_engine("redact", None, Some(model)).await;
        engine
            .session_manager
            .add_secrets_async(vec![secret.to_string()])
            .await
            .unwrap();

        let messages = vec![
            ChatMessage::user("Found an api key in branch"),
            ChatMessage::assistant("Logging key"),
        ];

        let summary = engine.summarize_branch(&messages).await;
        assert!(!summary.contains(secret));
        assert!(summary.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn rewind_keeps_an_unfinished_turn_in_the_tree() {
        let (engine, _dir) = test_engine("rewind", None, None).await;
        let session = &engine.session_manager;
        for n in 1..=2 {
            let turn = vec![
                ChatMessage::user(format!("prompt {n}")),
                ChatMessage::assistant(format!("answer {n}")),
            ];
            session.append_messages(&session.session_id, turn).await.unwrap();
        }
        let first_turn = session.load_messages().await.unwrap()[..2].to_vec();
        session
            .save_checkpoint(vec![ChatMessage::user("unfinished prompt")])
            .await
            .unwrap();

        let retained = engine.rewind_to_turn(1).await.unwrap();

        assert_eq!(retained, 2);
        assert_eq!(session.load_messages().await.unwrap(), first_turn);
        assert!(session.load_checkpoint().await.unwrap().is_none());
        let tree = session.load_tree().await.unwrap();
        assert!(
            tree.nodes
                .values()
                .any(|node| node.messages.first() == Some(&ChatMessage::user("unfinished prompt")))
        );
    }
}

mod compactor {
    use rho_harness_core::model::{AssistantContent, ChatMessage, ToolCall, ToolFunction};
    use std::sync::Arc;

    use crate::engine::compactor::llm::{LlmCompactor, SummarizeOptions};
    use crate::provider::adapter::MockAdapter;

    #[tokio::test]
    async fn test_llm_compactor_fallback_when_model_is_none() {
        let compactor = LlmCompactor::new(None);
        let messages = vec![
            rho_harness_core::model::ChatMessage::user("Please implement feature X in src/app.rs"),
            rho_harness_core::model::ChatMessage::assistant("I have completed feature X in src/app.rs."),
        ];

        let summary = compactor.summarize(&messages, SummarizeOptions::default()).await;
        assert!(summary.contains("## Goal"));
        assert!(summary.contains("src/app.rs"));
    }

    #[test]
    fn test_strip_media_for_summarization_replaces_image_with_stub() {
        use rho_harness_core::model::{ChatMessage, ToolResult, ToolResultContent, UserContent};

        let msg_with_image = ChatMessage::User {
            content: vec![
                UserContent::text("Here is an image:"),
                UserContent::image("iVBORw0KGgoAAAANSUhEUgA=", Some("image/png".to_string())),
                UserContent::ToolResult(ToolResult {
                    call: "c1".to_string(),
                    provider: None,
                    name: "screenshot".to_string(),
                    content: vec![
                        ToolResultContent::text("Captured screen"),
                        ToolResultContent::image("iVBORw0KGgoAAAANSUhEUgA=", Some("image/jpeg".to_string())),
                    ],
                }),
            ],
        };

        let stripped = crate::engine::compactor::llm::strip_media_for_summarization(&[msg_with_image]);
        assert_eq!(stripped.len(), 1);
        let ChatMessage::User { content } = &stripped[0] else {
            panic!()
        };
        assert_eq!(content.len(), 3);

        let UserContent::Text(t1) = &content[0] else { panic!() };
        assert_eq!(t1.text, "Here is an image:");

        let UserContent::Text(t2) = &content[1] else { panic!() };
        assert_eq!(t2.text, "[Image: image/png]");

        let UserContent::ToolResult(res) = &content[2] else {
            panic!()
        };
        assert_eq!(res.content.len(), 2);
        let ToolResultContent::Text(rt1) = &res.content[0] else {
            panic!()
        };
        assert_eq!(rt1.text, "Captured screen");
        let ToolResultContent::Text(rt2) = &res.content[1] else {
            panic!()
        };
        assert_eq!(rt2.text, "[Image: image/jpeg]");
    }

    #[tokio::test]
    async fn test_llm_compactor_successful_llm_call() {
        let mock = Arc::new(MockAdapter::text(
            "## Goal\nImplement feature Y\n\n## Progress\n### Done\n- [x] Done",
        ));
        let compactor = LlmCompactor::new(Some(mock.clone()));

        let messages = vec![
            ChatMessage::user("Please implement feature Y"),
            ChatMessage::assistant("Done feature Y"),
        ];

        let summary = compactor.summarize(&messages, SummarizeOptions::default()).await;
        assert_eq!(
            summary,
            "## Goal\nImplement feature Y\n\n## Progress\n### Done\n- [x] Done"
        );

        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].preamble.as_deref(),
            Some(rho_harness_core::session::compaction::SUMMARIZATION_SYSTEM_PROMPT)
        );
    }

    #[tokio::test]
    async fn test_llm_compactor_completion_failure_falls_back_to_deterministic_summary() {
        let mock = Arc::new(MockAdapter::default());
        mock.add_turn(vec![Err(rho_harness_core::error::AppError::Network(
            "connection reset".to_string(),
        ))]);
        let compactor = LlmCompactor::new(Some(mock));

        let messages = vec![
            ChatMessage::user("Critical task to summarize"),
            ChatMessage::assistant("Work in progress"),
        ];

        let summary = compactor.summarize(&messages, SummarizeOptions::default()).await;

        assert!(summary.contains("## Goal\nCritical task to summarize"));
        assert!(summary.contains("## Progress\n### Done"));
    }

    #[tokio::test]
    async fn test_llm_compactor_update_with_prior_summary() {
        let mock = Arc::new(MockAdapter::text("## Goal\nUpdated goal"));
        let compactor = LlmCompactor::new(Some(mock.clone()));

        let messages = vec![
            ChatMessage::user("Next step for task"),
            ChatMessage::assistant("Finished next step"),
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
        let mock = Arc::new(MockAdapter::text("## Early Progress\nPrefix work completed"));
        let compactor = LlmCompactor::new(Some(mock));

        let messages = vec![
            ChatMessage::user("Do huge operation"),
            ChatMessage::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(ToolCall::new(
                    "c1",
                    ToolFunction::new("read", serde_json::json!({"path": "src/main.rs"})),
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

    #[tokio::test]
    async fn test_llm_compactor_overflow_retries_and_recovers() {
        let mock = Arc::new(MockAdapter::default());
        mock.add_turn(vec![Err(rho_harness_core::error::AppError::ContextOverflow(
            "context_length_exceeded: maximum context length is 128000 tokens".to_string(),
        ))]);
        mock.add_turn(vec![Ok(crate::provider::adapter::ModelStreamEvent::Text(
            "## Goal\nRecovered after oldest round pruned\n\n## Progress\n### Done\n- [x] Succeeded".to_string(),
        ))]);
        let compactor = LlmCompactor::new(Some(mock.clone()));

        let messages = vec![
            ChatMessage::user("Turn 1 user request to be dropped on overflow"),
            ChatMessage::assistant("Turn 1 assistant response"),
            ChatMessage::user("Turn 2 user request that should be kept"),
            ChatMessage::assistant("Turn 2 assistant response"),
        ];

        let summary = compactor.summarize(&messages, SummarizeOptions::default()).await;
        assert_eq!(
            summary,
            "## Goal\nRecovered after oldest round pruned\n\n## Progress\n### Done\n- [x] Succeeded"
        );

        let requests = mock.requests();
        assert_eq!(requests.len(), 2);
        let first_prompt = format!("{:?}", requests[0]);
        let second_prompt = format!("{:?}", requests[1]);
        assert!(first_prompt.contains("Turn 1 user request"));
        assert!(!second_prompt.contains("Turn 1 user request"));
        assert!(second_prompt.contains("Turn 2 user request"));
    }

    #[tokio::test]
    async fn test_llm_compactor_overflow_falls_back_when_all_retries_exhausted() {
        let mock = Arc::new(MockAdapter::default());
        for _ in 0..4 {
            mock.add_turn(vec![Err(rho_harness_core::error::AppError::ContextOverflow(
                "context_length_exceeded".to_string(),
            ))]);
        }
        let compactor = LlmCompactor::new(Some(mock.clone()));

        let messages = vec![
            ChatMessage::user("Turn 1"),
            ChatMessage::assistant("Resp 1"),
            ChatMessage::user("Turn 2"),
            ChatMessage::assistant("Resp 2"),
            ChatMessage::user("Turn 3"),
            ChatMessage::assistant("Resp 3"),
            ChatMessage::user("Turn 4"),
            ChatMessage::assistant("Resp 4"),
            ChatMessage::user("Turn 5"),
            ChatMessage::assistant("Resp 5"),
        ];

        let summary = compactor.summarize(&messages, SummarizeOptions::default()).await;
        assert!(summary.contains("## Goal\nTurn 1"));
        assert!(summary.contains("## Progress\n### Done"));
        assert_eq!(mock.requests().len(), 4);
    }

    #[tokio::test]
    async fn test_llm_compactor_overflow_single_turn_falls_back_without_retrying() {
        let mock = Arc::new(MockAdapter::default());
        mock.add_turn(vec![Err(rho_harness_core::error::AppError::ContextOverflow(
            "context_length_exceeded".to_string(),
        ))]);
        let compactor = LlmCompactor::new(Some(mock.clone()));

        let messages = vec![
            ChatMessage::user("Only turn user prompt"),
            ChatMessage::assistant("Only turn assistant response"),
        ];

        let summary = compactor.summarize(&messages, SummarizeOptions::default()).await;
        assert!(summary.contains("## Goal\nOnly turn user prompt"));
        assert_eq!(mock.requests().len(), 1);
    }
}

mod common {
    use crate::auth::AuthStore;
    use crate::engine::AgentEngine;
    use crate::engine::builder::AgentEngineBuilder;
    use crate::provider::adapter::ModelAdapter;
    use crate::test_support::TestDir;
    use rho_harness_core::config::Config;
    use std::sync::Arc;

    pub async fn test_engine(
        label: &str,
        keep_recent_tokens: Option<usize>,
        model: Option<Arc<dyn ModelAdapter>>,
    ) -> (AgentEngine, TestDir) {
        let dir = TestDir::with_prefix(&format!("compactor_{label}_"));
        let mut config = Config {
            sessions_dir: dir.join("sessions"),
            auth_file: dir.join("auth.json"),
            ..Default::default()
        };
        if let Some(keep) = keep_recent_tokens {
            config.keep_recent_tokens = keep;
        }
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let mut builder = AgentEngineBuilder::new(config, auth_store)
            .base_dir(dir.to_path_buf())
            .tools(Vec::new());
        if let Some(m) = model {
            builder = builder.model(m);
        }
        (builder.build().await.unwrap(), dir)
    }

    pub async fn append_test_messages(
        sm: &rho_harness_core::session::SessionManager,
        sid: &str,
        msgs: Vec<rho_harness_core::model::ChatMessage>,
    ) {
        sm.append_messages(sid, msgs).await.unwrap();
    }
}

mod orchestrator {
    use super::common::{self, test_engine};
    use rho_harness_core::config::Config;
    use rho_harness_core::model::{AssistantContent, ChatMessage, ToolCall, ToolFunction, UserContent};
    use rho_harness_core::session::tree::TreeNodeKind;
    use std::sync::Arc;

    use crate::auth::AuthStore;
    use crate::engine::builder::AgentEngineBuilder;
    use crate::provider::adapter::MockAdapter;

    fn tool_turn(cid: &str, tool: &str, path: &str, user: &str, done: &str) -> Vec<ChatMessage> {
        let call = ToolCall::new(cid, ToolFunction::new(tool, serde_json::json!({"path": path})));
        vec![
            ChatMessage::user(user),
            ChatMessage::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call)],
            },
            ChatMessage::User {
                content: vec![UserContent::ToolResult(rho_harness_core::model::ToolResult::new(
                    cid, tool, "data",
                ))],
            },
            ChatMessage::assistant(done),
        ]
    }

    async fn populate_test_turns(sm: &rho_harness_core::session::SessionManager, sid: &str) {
        let turn1 = tool_turn("c1", "read", "Cargo.toml", "Read config", "Done read");
        let turn2 = tool_turn("c2", "write", "src/storage.rs", "Edit storage", "Done write");
        for turn in [
            turn1,
            turn2,
            vec![ChatMessage::user("Verify"), ChatMessage::assistant("Verified")],
        ] {
            common::append_test_messages(sm, sid, turn).await;
        }
    }

    #[tokio::test]
    async fn test_compact_session_with_file_tracking_and_metrics() {
        let mock = Arc::new(MockAdapter::text(
            "## Goal\nRefactor session storage\n\n## Progress\n### Done\n- [x] Read files",
        ));
        let (engine, _dir) = test_engine("file_tracking", Some(10), Some(mock)).await;
        let session_id = engine.session_manager.session_id.clone();
        populate_test_turns(&engine.session_manager, &session_id).await;

        let stats = engine.compact_session(Some("Focus on storage refactor")).await.unwrap();
        assert!(stats.tokens_before > 0);
        assert!(stats.tokens_after > 0);
        assert!(stats.summary.contains("Refactor session storage"));
        assert!(stats.summary.contains("<read-files>"));
        assert!(stats.summary.contains("Cargo.toml"));
        assert!(stats.summary.contains("<modified-files>"));
        assert!(stats.summary.contains("src/storage.rs"));

        let tree = engine.session_manager.load_tree().await.unwrap();
        let leaf_id = tree.active_leaf_id.as_ref().unwrap();
        let nodes = tree.ancestor_nodes(leaf_id);
        let comp_node = nodes.iter().find(|n| n.kind == TreeNodeKind::Compaction);
        assert!(comp_node.is_some());

        let meta = comp_node.unwrap().compaction_metadata().unwrap();
        assert_eq!(meta.custom_instructions.as_deref(), Some("Focus on storage refactor"));
        assert!(meta.read_files.contains(&"Cargo.toml".to_string()));
        assert!(meta.modified_files.contains(&"src/storage.rs".to_string()));

        let active_messages = tree.active_messages();
        assert!(matches!(
            &active_messages[0],
            rho_harness_core::model::ChatMessage::System { .. }
        ));
    }

    #[tokio::test]
    async fn test_compaction_usage_tracking_preserves_session_totals_and_updates_active_context() {
        use crate::engine::metrics::StructuralUsage;

        let mock = Arc::new(MockAdapter::text("## Summary\nPrior work completed"));
        let (engine, _dir) = test_engine("usage_preservation", Some(10), Some(mock)).await;
        let session_id = engine.session_manager.session_id.clone();
        populate_test_turns(&engine.session_manager, &session_id).await;

        let initial_usage = StructuralUsage {
            input_tokens: 10_000,
            output_tokens: 1_000,
            total_tokens: 11_000,
            ..Default::default()
        };
        engine.usage.record(initial_usage);
        let totals_before = engine.usage.totals();
        assert_eq!(totals_before.total_input, 10_000);

        let stats = engine.compact_session(None).await.unwrap();
        assert!(stats.tokens_after > 0);

        let latest = engine.usage.latest().expect("latest context updated");
        assert_eq!(latest.input_tokens, stats.tokens_after as u64);

        let totals_after = engine.usage.totals();
        assert_ne!(totals_after.total_input, 10_000 + stats.tokens_after as u64);
    }

    #[tokio::test]
    async fn compaction_leaves_the_prompt_cache_cold() {
        use crate::engine::metrics::types::{CompletionCall, StructuralUsage};

        let mock = Arc::new(MockAdapter::text("## Summary\nPrior work completed"));
        let (engine, _dir) = test_engine("compaction_cache", Some(10), Some(mock)).await;
        let session_id = engine.session_manager.session_id.clone();
        populate_test_turns(&engine.session_manager, &session_id).await;
        engine.run_tracker.completion(
            CompletionCall::new(0, StructuralUsage::default()),
            std::time::Instant::now(),
        );

        engine.compact_session(None).await.unwrap();

        assert!(engine.run_tracker.last_request().is_none());
    }

    #[tokio::test]
    async fn compaction_keeps_the_unfinished_todo_list_visible() {
        use rho_harness_core::args::{TodoItem, TodoStatus};
        use rho_harness_core::todo::{latest_todos, render_todos};

        let plan = vec![
            TodoItem {
                content: "Refactor storage".into(),
                status: TodoStatus::InProgress,
            },
            TodoItem {
                content: "Run the tests".into(),
                status: TodoStatus::Pending,
            },
        ];
        let mock = Arc::new(MockAdapter::text("## Summary\nPrior work completed"));
        let (engine, _dir) = test_engine("todo_carry_over", Some(10), Some(mock)).await;
        let sm = &engine.session_manager;
        let session_id = sm.session_id.clone();
        let call = ToolCall::new("todo-1", ToolFunction::new("todo", serde_json::json!({})));
        let todo_turn = vec![
            ChatMessage::user("Plan it"),
            ChatMessage::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call)],
            },
            ChatMessage::User {
                content: vec![UserContent::ToolResult(rho_harness_core::model::ToolResult::new(
                    "todo-1",
                    "todo",
                    render_todos(&plan),
                ))],
            },
            ChatMessage::assistant("Planned"),
        ];
        common::append_test_messages(sm, &session_id, todo_turn).await;
        populate_test_turns(sm, &session_id).await;

        let first = engine.compact_session(None).await.unwrap();
        populate_test_turns(sm, &session_id).await;
        let second = engine.compact_session(None).await.unwrap();

        let block = format!("<todos>\n{}\n</todos>", render_todos(&plan));
        assert!(first.summary.ends_with(&block), "{}", first.summary);
        assert!(second.summary.ends_with(&block), "{}", second.summary);
        let active = sm.load_tree().await.unwrap().active_messages();
        assert!(matches!(&active[0], ChatMessage::System { .. }));
        assert_eq!(latest_todos(&active), Some(plan));
    }

    #[tokio::test]
    async fn test_compact_session_empty_or_single_node() {
        let (engine, _dir) = test_engine("empty_session", Some(10), None).await;
        let stats = engine.compact_session(None).await.unwrap();
        assert_eq!(stats.saved_tokens, 0);

        let session_id = engine.session_manager.session_id.clone();
        common::append_test_messages(
            &engine.session_manager,
            &session_id,
            vec![ChatMessage::user("Hello"), ChatMessage::assistant("Hi")],
        )
        .await;

        let stats2 = engine.compact_session(None).await.unwrap();
        assert_eq!(stats2.saved_tokens, 0);
    }

    fn split_turn_fixture() -> Vec<ChatMessage> {
        vec![
            ChatMessage::user("Preamble prompt"),
            ChatMessage::assistant("Early reply"),
            ChatMessage::user("Tool result 1"),
            ChatMessage::assistant("Mid reply with many tokens to force cut point selection"),
            ChatMessage::user("Tool result 2"),
            ChatMessage::assistant("Final suffix reply"),
        ]
    }

    fn assert_preamble_omitted(messages: &[rho_harness_core::model::ChatMessage]) {
        assert!(matches!(
            &messages[0],
            rho_harness_core::model::ChatMessage::System { .. }
        ));
        assert!(!messages.iter().any(|m| match m {
            rho_harness_core::model::ChatMessage::User { content } => content.iter().any(|c| match c {
                rho_harness_core::model::UserContent::Text(t) => t.text.contains("Preamble prompt"),
                _ => false,
            }),
            _ => false,
        }));
    }

    #[derive(Default)]
    struct CapturingDemotionHook {
        demotions: std::sync::Mutex<Vec<(String, Vec<ChatMessage>)>>,
        fail: bool,
        panic: bool,
    }

    impl crate::engine::compactor::DemotionHook for CapturingDemotionHook {
        fn on_demote<'a>(
            &'a self,
            session_id: &'a str,
            messages: &'a [ChatMessage],
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
            Box::pin(async move {
                if self.panic {
                    panic!("simulated hook panic");
                }
                if self.fail {
                    return Err("simulated hook failure".to_string());
                }
                self.demotions
                    .lock()
                    .unwrap()
                    .push((session_id.to_string(), messages.to_vec()));
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn test_compact_session_forwards_evicted_messages_to_demotion_hook() {
        let hook = std::sync::Arc::new(CapturingDemotionHook::default());
        let dir = crate::test_support::TestDir::new();
        let config = Config {
            sessions_dir: dir.join("sessions"),
            auth_file: dir.join("auth.json"),
            keep_recent_tokens: 10,
            ..Default::default()
        };
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let engine = AgentEngineBuilder::new(config, auth_store)
            .base_dir(dir.to_path_buf())
            .tools(Vec::new())
            .demotion_hook(hook.clone())
            .build()
            .await
            .unwrap();

        let session_id = engine.session_manager.session_id.clone();
        populate_test_turns(&engine.session_manager, &session_id).await;

        let stats = engine.compact_session(None).await.unwrap();
        assert!(stats.tokens_before > 0);

        let demotions = hook.demotions.lock().unwrap();
        assert_eq!(demotions.len(), 1);
        assert_eq!(demotions[0].0, session_id);
        assert!(!demotions[0].1.is_empty());
    }

    #[tokio::test]
    async fn test_compact_session_failing_demotion_hook_does_not_abort_compaction() {
        let hook = std::sync::Arc::new(CapturingDemotionHook {
            fail: true,
            ..Default::default()
        });
        let dir = crate::test_support::TestDir::new();
        let config = Config {
            sessions_dir: dir.join("sessions"),
            auth_file: dir.join("auth.json"),
            keep_recent_tokens: 10,
            ..Default::default()
        };
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let engine = AgentEngineBuilder::new(config, auth_store)
            .base_dir(dir.to_path_buf())
            .tools(Vec::new())
            .demotion_hook(hook)
            .build()
            .await
            .unwrap();

        let session_id = engine.session_manager.session_id.clone();
        populate_test_turns(&engine.session_manager, &session_id).await;

        let stats = engine.compact_session(None).await.unwrap();
        assert!(stats.tokens_before > 0);
    }

    #[tokio::test]
    async fn test_compact_session_panicking_demotion_hook_does_not_abort_compaction() {
        let hook = std::sync::Arc::new(CapturingDemotionHook {
            panic: true,
            ..Default::default()
        });
        let dir = crate::test_support::TestDir::new();
        let config = Config {
            sessions_dir: dir.join("sessions"),
            auth_file: dir.join("auth.json"),
            keep_recent_tokens: 10,
            ..Default::default()
        };
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let engine = AgentEngineBuilder::new(config, auth_store)
            .base_dir(dir.to_path_buf())
            .tools(Vec::new())
            .demotion_hook(hook)
            .build()
            .await
            .unwrap();

        let session_id = engine.session_manager.session_id.clone();
        populate_test_turns(&engine.session_manager, &session_id).await;

        let stats = engine.compact_session(None).await.unwrap();
        assert!(stats.tokens_before > 0);
    }

    #[tokio::test]
    async fn a_failing_demotion_hook_error_is_returned_to_the_caller() {
        let hook: std::sync::Arc<dyn crate::engine::compactor::DemotionHook> =
            std::sync::Arc::new(CapturingDemotionHook {
                fail: true,
                ..Default::default()
            });

        let result =
            crate::engine::compactor::orchestrator::dispatch_demote(Some(&hook), "session", &[ChatMessage::user("hi")])
                .await;

        assert_eq!(result, Err("simulated hook failure".to_string()));
    }

    #[tokio::test]
    async fn a_panicking_demotion_hook_is_returned_to_the_caller_as_an_error() {
        let hook: std::sync::Arc<dyn crate::engine::compactor::DemotionHook> =
            std::sync::Arc::new(CapturingDemotionHook {
                panic: true,
                ..Default::default()
            });

        let result =
            crate::engine::compactor::orchestrator::dispatch_demote(Some(&hook), "session", &[ChatMessage::user("hi")])
                .await;

        assert_eq!(result, Err("demotion hook panicked".to_string()));
    }

    #[tokio::test]
    async fn test_compact_session_split_turn_prunes_prefix_messages() {
        let mock = Arc::new(MockAdapter::text("## Goal\nComplete huge operation"));
        let (engine, _dir) = test_engine("split_turn", Some(10), Some(mock)).await;
        let sid = engine.session_manager.session_id.clone();
        common::append_test_messages(&engine.session_manager, &sid, split_turn_fixture()).await;

        let stats = engine.compact_session(None).await.unwrap();
        assert!(stats.tokens_before > 0);

        let tree = engine.session_manager.load_tree().await.unwrap();
        let leaf_id = tree.active_leaf_id.as_ref().unwrap();
        let comp_node = tree
            .ancestor_nodes(leaf_id)
            .into_iter()
            .find(|n| n.kind == TreeNodeKind::Compaction)
            .unwrap();
        assert!(comp_node.compaction_metadata().unwrap().first_kept_node_id.is_some());
        assert_preamble_omitted(&tree.active_messages());
    }

    #[tokio::test]
    async fn test_compact_session_respects_compaction_max_bytes() {
        let dir = crate::test_support::TestDir::new();
        let config = Config {
            sessions_dir: dir.join("sessions"),
            auth_file: dir.join("auth.json"),
            keep_recent_tokens: 5,
            compaction_max_bytes: 120,
            ..Default::default()
        };
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let engine = AgentEngineBuilder::new(config, auth_store)
            .base_dir(dir.to_path_buf())
            .tools(Vec::new())
            .build()
            .await
            .unwrap();

        let sid = &engine.session_manager.session_id;
        let messages = vec![
            rho_harness_core::model::ChatMessage::user("Please build an entire large subsystem with lots of details."),
            rho_harness_core::model::ChatMessage::assistant(
                "I will now write multiple files and refactor the architecture comprehensively.",
            ),
        ];
        engine.session_manager.append_messages(sid, messages).await.unwrap();

        let stats = engine.compact_session(None).await.unwrap();
        assert!(stats.summary.len() <= 120);
    }
}

mod recovery {
    use super::common;
    use rho_harness_core::model::ChatMessage;
    use std::sync::Arc;

    use rho_harness_core::config::Config;
    use rho_harness_core::error::AppError;

    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::engine::runner::TurnRequest;
    use crate::provider::adapter::{MockAdapter, ModelStreamEvent};
    use crate::test_support::CapturingPresenter;

    async fn populate_recovery_history(sm: &rho_harness_core::session::SessionManager, sid: &str) {
        for i in 1..=2 {
            let msgs = vec![
                ChatMessage::user(format!("Old turn {i}")),
                ChatMessage::assistant(format!("Old response {i}")),
            ];
            common::append_test_messages(sm, sid, msgs).await;
        }
    }

    fn assert_recovery_presenter(presenter: &CapturingPresenter) {
        let notices = presenter.notices.lock().unwrap().clone();
        assert!(
            notices.iter().any(|n| n.contains("Auto-compacted context")),
            "{notices:?}"
        );
        assert!(
            presenter
                .spinner_messages
                .lock()
                .unwrap()
                .iter()
                .any(|m| m == "Compacting...")
        );
    }

    fn recovery_engine(dir: &std::path::Path, model: Arc<MockAdapter>) -> crate::engine::AgentEngine {
        engine_with(
            dir,
            model,
            Config {
                keep_recent_tokens: 5,
                auth_file: dir.join("auth.json"),
                ..Config::default()
            },
        )
    }

    fn engine_with(dir: &std::path::Path, model: Arc<MockAdapter>, app_config: Config) -> crate::engine::AgentEngine {
        mock_engine(model, MockEngineConfig::new(dir, app_config))
    }

    #[tokio::test]
    async fn compaction_summarizes_with_the_model_the_session_switched_to() {
        let dir = crate::test_support::TestDir::new();
        let first = Arc::new(MockAdapter::text("## Goal\nfrom the first model"));
        let mut engine = recovery_engine(&dir, first.clone());
        engine.compaction_summaries = crate::engine::compactor::CompactionSummaries::SessionModel;
        let session_id = engine.session_manager.session_id.clone();
        populate_recovery_history(&engine.session_manager, &session_id).await;
        let switched = Arc::new(MockAdapter::text("## Goal\nfrom the switched model"));
        engine.model = Some(switched.clone());

        let stats = engine.compact_session(None).await.unwrap();

        assert!(stats.summary.contains("from the switched model"), "{}", stats.summary);
        assert!(first.requests().is_empty());
        assert!(!switched.requests().is_empty());
    }

    #[tokio::test]
    async fn an_overflow_on_the_first_request_compacts_and_sends_the_prompt_again() {
        let dir = crate::test_support::TestDir::new();
        let model = Arc::new(MockAdapter::default());
        model.add_turn(vec![Err(AppError::ContextOverflow(
            "context_length_exceeded: maximum context length is 128000 tokens".to_string(),
        ))]);
        model.add_turn(vec![Ok(ModelStreamEvent::Text(
            "answered after compacting".to_string(),
        ))]);

        let engine = recovery_engine(&dir, model.clone());
        let session_id = engine.session_manager.session_id.clone();
        populate_recovery_history(&engine.session_manager, &session_id).await;

        let presenter = Arc::new(CapturingPresenter::default());
        let output = engine
            .run_turn(
                TurnRequest::new("New prompt that overflows initially"),
                presenter.clone(),
            )
            .await
            .unwrap();
        assert_eq!(output.status, crate::engine::metrics::TerminalStatus::Completed);
        assert_eq!(output.final_text, "answered after compacting");
        assert_recovery_presenter(&presenter);
        let last = model.requests().pop().unwrap();
        let asked = format!("{:?}", last.messages.last());
        assert!(asked.contains("New prompt that overflows initially"), "{asked}");
    }

    #[tokio::test]
    async fn overflow_after_checkpointed_steps_compacts_and_keeps_the_turn_work() {
        use rho_harness_core::model::{Reasoning, ToolCall, ToolFunction};
        let dir = crate::test_support::TestDir::new();
        let model = Arc::new(MockAdapter::default());
        model.add_turn(vec![
            Ok(ModelStreamEvent::ReasoningBlock(Reasoning::new_with_signature(
                "plan",
                Some("sig-1".into()),
            ))),
            Ok(ModelStreamEvent::ToolCall(ToolCall::new(
                "call-1",
                ToolFunction::new("missing", serde_json::json!({})),
            ))),
        ]);
        model.add_turn(vec![Err(AppError::ContextOverflow(
            "context_length_exceeded".to_string(),
        ))]);
        model.add_turn(vec![Ok(ModelStreamEvent::Text("finished the long task".to_string()))]);

        let engine = recovery_engine(&dir, model);
        let session_id = engine.session_manager.session_id.clone();
        populate_recovery_history(&engine.session_manager, &session_id).await;

        let presenter = Arc::new(CapturingPresenter::default());
        let output = engine
            .run_turn(TurnRequest::new("Long task that overflows mid-turn"), presenter.clone())
            .await;
        let notices = presenter.notices.lock().unwrap().clone();
        let output = output.unwrap_or_else(|e| panic!("{e}; notices: {notices:?}"));

        assert_eq!(output.final_text, "finished the long task");
        assert_eq!(
            output.requests, 2,
            "the step before the overflow and the answer after it"
        );
        assert_recovery_presenter(&presenter);
        let sm = &engine.session_manager;
        assert!(sm.load_checkpoint().await.unwrap().is_none());
        let mut messages = sm.load_messages().await.unwrap();
        let summary = format!("{:?}", messages.remove(0));
        let kept = format!("{messages:?}");
        let everything = format!("{summary}{kept}");
        assert_eq!(
            everything.matches("Long task that overflows mid-turn").count(),
            1,
            "{everything}"
        );
        assert!(
            everything.contains("This turn stopped before finishing"),
            "{everything}"
        );
        assert!(
            everything.contains("continue the task from where it stopped"),
            "{everything}"
        );
    }

    /// A step whose prompt used 9k tokens, past where a 10k window with a 2k reserve compacts.
    fn step_near_the_window(id: usize) -> Vec<Result<ModelStreamEvent, AppError>> {
        let usage = crate::engine::metrics::StructuralUsage {
            input_tokens: 9_000,
            ..Default::default()
        };
        crate::test_support::tool_step(format!("call-{id}"), "missing", serde_json::json!({}), usage)
            .into_iter()
            .map(Ok)
            .collect()
    }

    /// An engine on a 10k window that compacts past 8k.
    fn small_window_engine(dir: &std::path::Path, model: Arc<MockAdapter>) -> crate::engine::AgentEngine {
        let config = Config {
            keep_recent_tokens: 5,
            context_limit: Some(10_000),
            reserve_tokens: 2_000,
            auth_file: dir.join("auth.json"),
            ..Config::default()
        };
        engine_with(dir, model, config)
    }

    #[tokio::test]
    async fn a_turn_compacts_once_its_context_passes_the_threshold_without_waiting_for_an_overflow() {
        let dir = crate::test_support::TestDir::new();
        let model = Arc::new(MockAdapter::default());
        model.add_turn(step_near_the_window(1));
        model.add_turn(vec![Ok(ModelStreamEvent::Text("done without overflowing".to_string()))]);
        let engine = small_window_engine(&dir, model.clone());
        let session_id = engine.session_manager.session_id.clone();
        populate_recovery_history(&engine.session_manager, &session_id).await;

        let presenter = Arc::new(CapturingPresenter::default());
        let output = engine
            .run_turn(TurnRequest::new("Long task"), presenter.clone())
            .await
            .unwrap();

        assert_eq!(output.final_text, "done without overflowing");
        assert_recovery_presenter(&presenter);
        let requests = model.requests();
        assert_eq!(
            requests.len(),
            2,
            "the step, then the answer from the compacted context"
        );
        let resumed = &requests[1];
        assert!(
            matches!(&resumed.messages[0], ChatMessage::System { .. }),
            "{:?}",
            resumed.messages[0]
        );
        assert!(
            matches!(resumed.messages.last(), Some(ChatMessage::User { .. }))
                && format!("{:?}", resumed.messages.last()).contains("continue the task")
        );
    }

    #[tokio::test]
    async fn past_its_compaction_limit_an_early_stop_runs_on_from_its_saved_work() {
        use rho_harness_core::model::UserContent;
        let dir = crate::test_support::TestDir::new();
        let model = Arc::new(MockAdapter::default());
        let steps = crate::engine::compactor::MAX_COMPACTIONS_PER_TURN + 1;
        for i in 0..steps {
            model.add_turn(step_near_the_window(i));
        }
        model.add_turn(vec![Ok(ModelStreamEvent::Text("ran on".to_string()))]);
        let engine = small_window_engine(&dir, model.clone());

        let output = engine
            .run_turn(TurnRequest::new("Long task"), Arc::new(CapturingPresenter::default()))
            .await
            .unwrap();

        assert_eq!(output.final_text, "ran on");
        let requests = model.requests();
        assert_eq!(requests.len(), steps + 1, "every step, then the answer");
        let last = requests.last().unwrap();
        let resumed = format!("{:?}", last.messages);
        assert!(
            matches!(
                last.messages.last(),
                Some(ChatMessage::User { content }) if matches!(content.first(), Some(UserContent::ToolResult(_)))
            ),
            "it goes on from the tool result, with no resume prompt: {resumed}"
        );
    }

    #[tokio::test]
    async fn manual_compaction_continues_past_a_failed_turn_checkpoint() {
        let dir = crate::test_support::TestDir::new();
        let engine = recovery_engine(&dir, Arc::new(MockAdapter::default()));
        let sm = &engine.session_manager;
        populate_recovery_history(sm, &sm.session_id.clone()).await;
        sm.extend_checkpoint(vec![
            ChatMessage::user("Unfinished request"),
            ChatMessage::assistant("Partial answer"),
        ])
        .await
        .unwrap();

        let stats = engine.compact_session(None).await.unwrap();

        assert!(!stats.summary.is_empty());
        assert!(sm.load_checkpoint().await.unwrap().is_none());
        let history = format!("{:?}", sm.load_messages().await.unwrap());
        assert!(history.contains("Unfinished request"), "{history}");
        assert!(
            !history.contains("This turn stopped before finishing"),
            "turns ending in an answer need no note"
        );
    }

    #[tokio::test]
    async fn an_overflow_that_compaction_cannot_fix_fails_the_turn() {
        let dir = crate::test_support::TestDir::new();
        let model = Arc::new(MockAdapter::default());
        for _ in 0..crate::engine::compactor::MAX_COMPACTIONS_PER_TURN + 1 {
            model.add_turn(vec![Err(AppError::ContextOverflow(
                "context_length_exceeded".to_string(),
            ))]);
        }

        let engine = recovery_engine(&dir, model);
        let session_id = engine.session_manager.session_id.clone();
        populate_recovery_history(&engine.session_manager, &session_id).await;

        let presenter = Arc::new(CapturingPresenter::default());
        let result = engine
            .run_turn(TurnRequest::new("Persistent overflow prompt"), presenter)
            .await;
        assert!(matches!(result, Err(AppError::ContextOverflow(_))), "{result:?}");
    }
}

mod sequential {
    use super::common::{self, test_engine};
    use rho_harness_core::model::{AssistantContent, ChatMessage, ToolCall, ToolFunction, UserContent};
    use rho_harness_core::session::tree::TreeNodeKind;
    use std::sync::Arc;

    use crate::provider::adapter::MockAdapter;

    fn file_turn(call_id: &str, tool: &str, path: &str) -> Vec<ChatMessage> {
        let call = ToolCall::new(call_id, ToolFunction::new(tool, serde_json::json!({"path": path})));
        vec![
            ChatMessage::user(format!("Execute {tool} on {path}")),
            ChatMessage::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call)],
            },
            ChatMessage::User {
                content: vec![UserContent::ToolResult(rho_harness_core::model::ToolResult::new(
                    call_id, tool, "done",
                ))],
            },
            ChatMessage::assistant(format!("Completed {tool} on {path}.")),
        ]
    }

    async fn append_file_turns(
        sm: &rho_harness_core::session::SessionManager,
        sid: &str,
        turns: &[(&str, &str, &str)],
    ) {
        for (call_id, tool, path) in turns {
            let turn = file_turn(call_id, tool, path);
            common::append_test_messages(sm, sid, turn).await;
        }
    }

    async fn assert_two_compactions(sm: &rho_harness_core::session::SessionManager) {
        let tree = sm.load_tree().await.unwrap();
        let leaf_id = tree.active_leaf_id.as_ref().unwrap();
        let count = tree
            .ancestor_nodes(leaf_id)
            .into_iter()
            .filter(|n| n.kind == TreeNodeKind::Compaction)
            .count();
        assert_eq!(count, 2);
    }

    fn assert_files_tracked(summary: &str) {
        for f in ["file1.txt", "file2.txt", "file3.txt", "file4.txt"] {
            assert!(summary.contains(f));
        }
    }

    fn assert_no_system_messages_in_prompts(requests: &[crate::provider::adapter::ModelCompletionRequest]) {
        assert!(requests.len() >= 2);
        for req in &requests[1..] {
            assert!(req.messages.iter().all(|m| !m.is_system()));
        }
    }

    #[tokio::test]
    async fn test_sequential_compactions_accumulate_files() {
        let mock = Arc::new(MockAdapter::default());
        mock.add_turn(vec![Ok(crate::provider::adapter::ModelStreamEvent::Text(
            "## Goal\nFirst compaction".to_string(),
        ))]);
        mock.add_turn(vec![Ok(crate::provider::adapter::ModelStreamEvent::Text(
            "## Goal\nPrefix 1".to_string(),
        ))]);
        mock.add_turn(vec![Ok(crate::provider::adapter::ModelStreamEvent::Text(
            "## Goal\nSecond compaction".to_string(),
        ))]);
        mock.add_turn(vec![Ok(crate::provider::adapter::ModelStreamEvent::Text(
            "## Goal\nPrefix 2".to_string(),
        ))]);

        let (engine, _dir) = test_engine("seq", Some(10), Some(mock.clone())).await;
        let sid = engine.session_manager.session_id.clone();

        append_file_turns(
            &engine.session_manager,
            &sid,
            &[("c1", "read", "file1.txt"), ("c2", "write", "file2.txt")],
        )
        .await;
        let s1 = engine.compact_session(None).await.unwrap();
        assert!(s1.summary.contains("file1.txt") && s1.summary.contains("file2.txt"));

        append_file_turns(
            &engine.session_manager,
            &sid,
            &[("c3", "edit", "file3.txt"), ("c4", "read", "file4.txt")],
        )
        .await;
        let s2 = engine.compact_session(None).await.unwrap();
        assert_files_tracked(&s2.summary);
        assert_two_compactions(&engine.session_manager).await;
        assert_no_system_messages_in_prompts(&mock.requests());
    }
}

mod auto_compact {
    use super::common;
    use rho_harness_core::model::ChatMessage;
    use std::sync::Arc;

    use rho_harness_core::config::Config;
    use rho_harness_core::model::{ToolCall, ToolFunction};
    use rho_harness_core::session::tree::TreeNodeKind;

    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::engine::runner::TurnRequest;
    use crate::provider::adapter::MockAdapter;
    use crate::test_support::CapturingPresenter;

    async fn populate_proactive_history(sm: &rho_harness_core::session::SessionManager, sid: &str) {
        for i in 1..=2 {
            let msgs = vec![
                ChatMessage::user(format!("Turn {i} request with many tokens")),
                ChatMessage::assistant(format!("Turn {i} response")),
            ];
            common::append_test_messages(sm, sid, msgs).await;
        }
    }

    fn assert_compaction_presenter(presenter: &CapturingPresenter) {
        let notices = presenter.notices.lock().unwrap().clone();
        assert!(notices.iter().any(|n| n.contains("Auto-compacted context")));
        assert!(
            presenter
                .spinner_messages
                .lock()
                .unwrap()
                .iter()
                .any(|m| m == "Compacting...")
        );
    }

    fn proactive_engine(dir: &std::path::Path, reserve_tokens: usize) -> crate::engine::AgentEngine {
        let app_config = Config {
            model: "mock-model".to_string(),
            reserve_tokens,
            keep_recent_tokens: 5,
            auth_file: dir.join("auth.json"),
            ..Config::default()
        };
        let model = Arc::new(MockAdapter::text("turn response"));
        mock_engine(model, MockEngineConfig::new(dir, app_config))
    }

    async fn assert_compaction_node_present(sm: &rho_harness_core::session::SessionManager) {
        let tree = sm.load_tree().await.unwrap();
        let leaf_id = tree.active_leaf_id.as_ref().unwrap();
        assert!(
            tree.ancestor_nodes(leaf_id)
                .iter()
                .any(|n| n.kind == TreeNodeKind::Compaction)
        );
    }

    #[tokio::test]
    async fn a_turn_that_compacts_before_starting_still_sends_its_prompt() {
        let dir = crate::test_support::TestDir::new();
        let engine = proactive_engine(&dir, 127_980);
        let session_id = engine.session_manager.session_id.clone();
        populate_proactive_history(&engine.session_manager, &session_id).await;

        let presenter = Arc::new(CapturingPresenter::default());
        let output = engine
            .run_turn(TurnRequest::new("Turn 3 request"), presenter.clone())
            .await
            .unwrap();
        assert_eq!(output.status, crate::engine::metrics::TerminalStatus::Completed);
        assert_eq!(output.final_text, "turn response");
        assert_eq!(output.requests, 1);
        assert_compaction_presenter(&presenter);
        assert_compaction_node_present(&engine.session_manager).await;
    }

    fn record_test_turn_usage(engine: &crate::engine::AgentEngine, tokens: u64) {
        let u = crate::engine::metrics::StructuralUsage {
            input_tokens: tokens,
            output_tokens: 0,
            total_tokens: tokens,
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            tool_use_prompt_tokens: None,
            reasoning_tokens: None,
        };
        engine
            .usage
            .record_turn(crate::engine::tracking::TurnUsage::new(u, u), 100);
    }

    async fn check_compaction_step(
        engine: &crate::engine::AgentEngine,
        presenter: &CapturingPresenter,
        history: &[rho_harness_core::model::ChatMessage],
        tokens: u64,
    ) {
        record_test_turn_usage(engine, tokens);
        engine.compact_if_full(presenter, history, 0, false).await.unwrap();
    }

    fn threshold_engine(dir: &std::path::Path) -> crate::engine::AgentEngine {
        let app_config = Config {
            model: "mock-model".to_string(),
            keep_recent_tokens: 5,
            auth_file: dir.join("auth.json"),
            ..Config::default()
        };
        let model = Arc::new(MockAdapter::text("response"));
        mock_engine(model, MockEngineConfig::new(dir, app_config))
    }

    async fn seed_threshold_history(engine: &crate::engine::AgentEngine) -> Vec<rho_harness_core::model::ChatMessage> {
        let sid = &engine.session_manager.session_id;
        let msgs = vec![
            ChatMessage::user("prior prompt"),
            ChatMessage::assistant("prior response"),
        ];
        common::append_test_messages(&engine.session_manager, sid, msgs).await;
        engine.session_manager.load_messages().await.unwrap()
    }

    async fn seed_large_read_history(engine: &crate::engine::AgentEngine) {
        use rho_harness_core::model::{AssistantContent, TextContent, ToolResult, ToolResultContent, UserContent};
        let body = (0..500)
            .map(|i| format!("line {i} {}", "x".repeat(90)))
            .collect::<Vec<_>>()
            .join("\n");
        let sid = engine.session_manager.session_id.clone();
        for i in 0..10 {
            let call = ToolCall::new(
                format!("c{i}"),
                ToolFunction::new(
                    "read".to_string(),
                    serde_json::json!({ "path": format!("src/f{i}.rs") }),
                ),
            );
            let result = ToolResult {
                call: format!("c{i}"),
                provider: None,
                name: "read".to_string(),
                content: vec![ToolResultContent::Text(TextContent::new(&body))],
            };
            let msgs = vec![
                ChatMessage::user(format!("task {i}")),
                ChatMessage::Assistant {
                    id: None,
                    content: vec![AssistantContent::ToolCall(call)],
                },
                ChatMessage::User {
                    content: vec![UserContent::ToolResult(result)],
                },
                ChatMessage::assistant(format!("done {i}")),
            ];
            common::append_test_messages(&engine.session_manager, &sid, msgs).await;
        }
    }

    #[tokio::test]
    async fn test_stale_usage_does_not_compact_history_that_pruning_shrank() {
        let dir = crate::test_support::TestDir::new();
        let engine = threshold_engine(&dir);
        seed_large_read_history(&engine).await;
        record_test_turn_usage(&engine, 120_000);

        let presenter = Arc::new(CapturingPresenter::default());
        let output = engine
            .run_turn(TurnRequest::new("next"), presenter.clone())
            .await
            .unwrap();
        assert_eq!(output.status, crate::engine::metrics::TerminalStatus::Completed);
        assert!(presenter.notices.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_compaction_limit_matches_pruning_limit_without_model_store_entry() {
        let dir = crate::test_support::TestDir::new();
        let app_config = Config {
            model: "claude-sonnet-4".to_string(),
            provider: "claude".to_string(),
            config_dir: dir.to_path_buf(),
            auth_file: dir.join("auth.json"),
            ..Config::default()
        };
        let engine = mock_engine(
            Arc::new(MockAdapter::text("response")),
            MockEngineConfig::new(&dir, app_config),
        );
        let history = seed_threshold_history(&engine).await;
        let presenter = Arc::new(CapturingPresenter::default());

        check_compaction_step(&engine, &presenter, &history, 150_000).await;
        assert!(presenter.notices.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_proactive_auto_compaction_at_reserve_threshold() {
        let dir = crate::test_support::TestDir::new();
        let engine = threshold_engine(&dir);
        let history = seed_threshold_history(&engine).await;
        let presenter = Arc::new(CapturingPresenter::default());

        // Default reserve of 16,384 against a 128k window triggers at 111,617.
        check_compaction_step(&engine, &presenter, &history, 111_616).await;
        assert!(presenter.notices.lock().unwrap().is_empty());
        check_compaction_step(&engine, &presenter, &history, 111_617).await;
        let notices = presenter.notices.lock().unwrap().clone();
        assert!(notices.iter().any(|n| n.contains("Auto-compacted context")));
    }

    #[tokio::test]
    async fn test_proactive_compaction_counts_cached_prompt_tokens() {
        let dir = crate::test_support::TestDir::new();
        let engine = threshold_engine(&dir);
        let history = seed_threshold_history(&engine).await;
        let presenter = Arc::new(CapturingPresenter::default());
        let usage = crate::engine::metrics::StructuralUsage {
            input_tokens: 1_617,
            cached_input_tokens: Some(100_000),
            cache_creation_input_tokens: Some(10_000),
            ..Default::default()
        };
        engine
            .usage
            .record_turn(crate::engine::tracking::TurnUsage::single(usage), 100);

        engine
            .compact_if_full(presenter.as_ref(), &history, 0, false)
            .await
            .unwrap();
        let notices = presenter.notices.lock().unwrap().clone();
        assert!(notices.iter().any(|n| n.contains("Auto-compacted context")));
    }
}

mod turn_compaction {
    use crate::engine::compactor::{MAX_COMPACTIONS_PER_TURN, MadeRoom, TurnCompaction};
    use crate::engine::tracking::CompactionTiming;
    use rho_harness_core::error::AppError;
    use std::convert::Infallible;

    fn early_stop() -> AppError {
        AppError::CompactionDue { used: 1 }
    }

    fn overflow() -> AppError {
        AppError::ContextOverflow("too long".into())
    }

    async fn make_room(turn: &mut TurnCompaction, error: AppError, folds: bool) -> Option<MadeRoom<()>> {
        let Ok(made) = turn
            .make_room(&error, async || Ok::<_, Infallible>(folds.then_some(())))
            .await;
        made
    }

    #[tokio::test]
    async fn a_full_context_compacts_or_runs_on_and_other_errors_end_the_turn() {
        let cases = [
            (AppError::Network("reset".into()), true, None, CompactionTiming::Early),
            (
                early_stop(),
                true,
                Some(MadeRoom::Compacted(())),
                CompactionTiming::Early,
            ),
            (overflow(), true, Some(MadeRoom::Compacted(())), CompactionTiming::Early),
            (early_stop(), false, Some(MadeRoom::RunOn), CompactionTiming::OnOverflow),
            (overflow(), false, None, CompactionTiming::Early),
        ];
        for (error, folds, expected, timing) in cases {
            let mut turn = TurnCompaction::default();
            let label = format!("{error:?} folds={folds}");
            assert_eq!(make_room(&mut turn, error, folds).await, expected, "{label}");
            assert_eq!(turn.timing(), timing, "{label}");
            assert_eq!(
                turn.has_compacted(),
                matches!(expected, Some(MadeRoom::Compacted(_))),
                "{label}"
            );
        }
    }

    #[tokio::test]
    async fn a_compaction_after_running_on_turns_early_stops_back_on() {
        let mut turn = TurnCompaction::default();
        make_room(&mut turn, early_stop(), false).await;
        assert_eq!(turn.timing(), CompactionTiming::OnOverflow);

        make_room(&mut turn, overflow(), true).await;

        assert_eq!(turn.timing(), CompactionTiming::Early);
    }

    #[tokio::test]
    async fn past_its_compaction_limit_a_turn_runs_on_until_an_overflow_ends_it() {
        let mut turn = TurnCompaction::default();
        for _ in 0..MAX_COMPACTIONS_PER_TURN {
            assert_eq!(
                make_room(&mut turn, early_stop(), true).await,
                Some(MadeRoom::Compacted(()))
            );
        }

        assert_eq!(make_room(&mut turn, early_stop(), true).await, Some(MadeRoom::RunOn));
        assert_eq!(make_room(&mut turn, overflow(), true).await, None);
    }
}
