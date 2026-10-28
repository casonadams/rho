//! Compactor tests: LLM summarization, orchestration, overflow recovery,
//! proactive auto-compaction, and sequential compaction accumulation.

mod branch {
    use rho_harness_core::config::Config;
    use rho_harness_core::model::ChatMessage;
    use std::sync::Arc;

    use crate::auth::AuthStore;
    use crate::engine::AgentEngine;
    use crate::engine::builder::AgentEngineBuilder;
    use crate::provider::adapter::{MockAdapter, ModelAdapter};

    async fn test_engine(label: &str, model: Option<Arc<dyn ModelAdapter>>) -> AgentEngine {
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
            builder = builder.model(m);
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
        let engine = test_engine("llm", Some(model)).await;

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
        let engine = test_engine("redact", Some(model)).await;
        engine.session_manager.add_secrets(vec![secret.to_string()]).unwrap();

        let messages = vec![
            ChatMessage::user("Found an api key in branch"),
            ChatMessage::assistant("Logging key"),
        ];

        let summary = engine.summarize_branch(&messages).await;
        assert!(!summary.contains(secret));
        assert!(summary.contains("[REDACTED]"));
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
    async fn test_llm_compactor_structured_extract() {
        #[derive(Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema, PartialEq, Eq)]
        struct TestMetadata {
            goal: String,
            files: Vec<String>,
        }

        let mock = Arc::new(MockAdapter::text(r#"{"goal": "Fix bug", "files": ["src/main.rs"]}"#));
        let compactor = LlmCompactor::new(Some(mock));

        let res: Option<TestMetadata> = compactor.extract("extract summary").await;
        assert_eq!(
            res,
            Some(TestMetadata {
                goal: "Fix bug".to_string(),
                files: vec!["src/main.rs".to_string()],
            })
        );
    }

    #[tokio::test]
    async fn test_llm_compactor_structured_extract_and_render() {
        let mock = Arc::new(MockAdapter::text(
            serde_json::to_string(&serde_json::json!({
                "goals": ["Build fast CLI"],
                "decisions": ["Use Rust"],
                "completed": ["Scaffolded repo"],
                "active_files": ["src/main.rs"],
                "open_questions": ["Add tests"],
            }))
            .unwrap(),
        ));
        let compactor = LlmCompactor::new(Some(mock));

        let messages = vec![
            ChatMessage::user("Please build fast CLI"),
            ChatMessage::assistant("I scaffolded the repo"),
        ];

        let summary = compactor
            .summarize(
                &messages,
                SummarizeOptions {
                    structured: true,
                    ..Default::default()
                },
            )
            .await;

        assert!(summary.contains("## Goals\n- Build fast CLI"));
        assert!(summary.contains("## Key Decisions\n- Use Rust"));
        assert!(summary.contains("## Completed Work\n- Scaffolded repo"));
        assert!(summary.contains("## Active Files\n- src/main.rs"));
        assert!(summary.contains("## Open Questions\n- Add tests"));
    }

    #[tokio::test]
    async fn test_llm_compactor_invalid_extraction_falls_back_to_completion() {
        let mock = Arc::new(MockAdapter::default());
        mock.add_turn(vec![Ok(crate::provider::adapter::ModelStreamEvent::Text(
            "invalid json causing extraction fallback".to_string(),
        ))]);
        mock.add_turn(vec![Ok(crate::provider::adapter::ModelStreamEvent::Text(
            "## Goal\nFallback completion goal\n\n## Progress\n### Done\n- [x] Done item".to_string(),
        ))]);
        let compactor = LlmCompactor::new(Some(mock));

        let messages = vec![
            ChatMessage::user("Please test fallback"),
            ChatMessage::assistant("Will do"),
        ];

        let summary = compactor
            .summarize(
                &messages,
                SummarizeOptions {
                    structured: true,
                    ..Default::default()
                },
            )
            .await;

        assert_eq!(
            summary,
            "## Goal\nFallback completion goal\n\n## Progress\n### Done\n- [x] Done item"
        );
    }

    #[tokio::test]
    async fn test_llm_compactor_completion_failure_falls_back_to_deterministic_summary() {
        let mock = Arc::new(MockAdapter::default());
        mock.add_turn(vec![Ok(crate::provider::adapter::ModelStreamEvent::Text(
            "non-json text causing extraction failure".to_string(),
        ))]);
        mock.add_turn(vec![Ok(
            crate::provider::adapter::ModelStreamEvent::Text(String::new()),
        )]);
        let compactor = LlmCompactor::new(Some(mock));

        let messages = vec![
            ChatMessage::user("Critical task to summarize"),
            ChatMessage::assistant("Work in progress"),
        ];

        let summary = compactor
            .summarize(
                &messages,
                SummarizeOptions {
                    structured: true,
                    ..Default::default()
                },
            )
            .await;

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
                    structured: false,
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
        mock.add_turn(vec![Err(rho_harness_core::error::AppError::Provider(
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
            mock.add_turn(vec![Err(rho_harness_core::error::AppError::Provider(
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
        mock.add_turn(vec![Err(rho_harness_core::error::AppError::Provider(
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
    use std::sync::Mutex;

    use async_trait::async_trait;
    use rho_harness_core::presentation::activity::ActivityToken;
    use rho_harness_core::presentation::presenter::Presenter;
    use rho_harness_core::presentation::stream::ToolStreamPort;
    use rho_harness_core::presentation::{SessionStatus, ToolLine, WelcomeDisplay};

    pub async fn append_test_messages(
        sm: &rho_harness_core::session::SessionManager,
        sid: &str,
        msgs: Vec<rho_harness_core::model::ChatMessage>,
    ) {
        sm.append_messages(sid, msgs).await.unwrap();
    }

    #[derive(Default)]
    pub struct CapturingPresenter {
        pub notices: Mutex<Vec<String>>,
        pub spinner_messages: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Presenter for CapturingPresenter {
        fn write_output(&self, _text: &str) {}
        fn print_welcome(&self, _display: &WelcomeDisplay) {}
        fn print_session_status(&self, _display: &SessionStatus) {}
        fn print_notice(&self, text: &str) {
            self.notices.lock().unwrap().push(text.to_string());
        }
        fn print_user_block(&self, _input: &str) {}
        fn print_token(&self, _token: &str) {}
        fn print_thinking_token(&self, _token: &str) {}
        fn finish_tool_line(&self, _line: ToolLine) {}
        fn flush(&self) {}
        fn has_interactive_ui(&self) -> bool {
            false
        }
        fn start_spinner(&self, message: &str) -> ActivityToken {
            self.spinner_messages.lock().unwrap().push(message.to_string());
            ActivityToken::default()
        }
        fn start_tool_spinner(&self, _name: &str, _arguments: &serde_json::Value) -> ActivityToken {
            ActivityToken::default()
        }
        fn start_tool_run(&self, _name: &str, _arguments: &serde_json::Value) {}
        fn stream_port(&self) -> ToolStreamPort {
            ToolStreamPort::default()
        }
        async fn prompt_continue_budget(&self, _max_turns: usize) -> bool {
            false
        }
    }
}

mod orchestrator {
    use super::common;
    use rho_harness_core::config::Config;
    use rho_harness_core::model::{AssistantContent, ChatMessage, ToolCall, ToolFunction, UserContent};
    use rho_harness_core::session::tree::TreeNodeKind;
    use std::sync::Arc;

    use crate::auth::AuthStore;
    use crate::engine::builder::AgentEngineBuilder;
    use crate::provider::adapter::MockAdapter;

    async fn test_engine(label: &str, model: Option<Arc<MockAdapter>>) -> crate::engine::AgentEngine {
        let dir = std::env::temp_dir().join(format!("orchestrator_{label}_{}", uuid::Uuid::new_v4()));
        let config = Config {
            sessions_dir: dir.join("sessions"),
            auth_file: dir.join("auth.json"),
            keep_recent_tokens: 10,
            ..Default::default()
        };
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let mut builder = AgentEngineBuilder::new(config, auth_store)
            .base_dir(dir)
            .tools(Vec::new());
        if let Some(m) = model {
            builder = builder.model(m);
        }
        builder.build().await.unwrap()
    }

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
        let engine = test_engine("file_tracking", Some(mock)).await;
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
        let engine = test_engine("usage_preservation", Some(mock)).await;
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
    async fn test_compact_session_empty_or_single_node() {
        let engine = test_engine("empty_session", None).await;
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
        let dir = std::env::temp_dir().join(format!("demote_hook_{}", uuid::Uuid::new_v4()));
        let config = Config {
            sessions_dir: dir.join("sessions"),
            auth_file: dir.join("auth.json"),
            keep_recent_tokens: 10,
            ..Default::default()
        };
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let engine = AgentEngineBuilder::new(config, auth_store)
            .base_dir(dir)
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
        let dir = std::env::temp_dir().join(format!("demote_fail_{}", uuid::Uuid::new_v4()));
        let config = Config {
            sessions_dir: dir.join("sessions"),
            auth_file: dir.join("auth.json"),
            keep_recent_tokens: 10,
            ..Default::default()
        };
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let engine = AgentEngineBuilder::new(config, auth_store)
            .base_dir(dir)
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
        let dir = std::env::temp_dir().join(format!("demote_panic_{}", uuid::Uuid::new_v4()));
        let config = Config {
            sessions_dir: dir.join("sessions"),
            auth_file: dir.join("auth.json"),
            keep_recent_tokens: 10,
            ..Default::default()
        };
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let engine = AgentEngineBuilder::new(config, auth_store)
            .base_dir(dir)
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
    async fn test_compact_session_split_turn_prunes_prefix_messages() {
        let mock = Arc::new(MockAdapter::text("## Goal\nComplete huge operation"));
        let engine = test_engine("split_turn", Some(mock)).await;
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
        let dir = std::env::temp_dir().join(format!("orchestrator_max_bytes_{}", uuid::Uuid::new_v4()));
        let config = Config {
            sessions_dir: dir.join("sessions"),
            auth_file: dir.join("auth.json"),
            keep_recent_tokens: 5,
            compaction_max_bytes: 120,
            ..Default::default()
        };
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let engine = AgentEngineBuilder::new(config, auth_store)
            .base_dir(dir.clone())
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
        let _ = std::fs::remove_dir_all(&dir);
    }
}

mod overflow {
    use crate::engine::compactor::{is_context_overflow_error, is_context_overflow_message};
    use rho_harness_core::error::AppError;

    #[test]
    fn test_is_context_overflow_message_patterns() {
        let overflow_messages = [
            "InvalidRequestError: This model's maximum context length is 128000 tokens.",
            "error: prompt is too long: 205000 tokens > 200000 maximum tokens",
            "ResourceExhausted: input token count exceeds limit",
            "context_length_exceeded",
            "context window exceeded",
            "Request payload size exceeds the limit: 1048576 bytes",
            "exceeds the context window of 128000 tokens",
        ];
        for msg in overflow_messages {
            assert!(is_context_overflow_message(msg));
        }
        for msg in [
            "Connection reset by peer",
            "Unauthorized 401",
            "Internal server error 500",
            "Rate limit exceeded 429",
        ] {
            assert!(!is_context_overflow_message(msg));
        }
    }

    #[test]
    fn test_is_context_overflow_streaming_error() {
        let completion_err = AppError::Provider("prompt is too long: 210000 tokens".to_string());
        assert!(is_context_overflow_error(&completion_err));

        let prompt_err = AppError::Provider("context_length_exceeded".to_string());
        assert!(is_context_overflow_error(&prompt_err));

        let unrelated = AppError::Provider("Model overloaded, try again later".to_string());
        assert!(!is_context_overflow_error(&unrelated));
    }
}

mod recovery {
    use super::common;
    use rho_harness_core::model::ChatMessage;
    use std::sync::Arc;

    use rho_harness_core::config::Config;
    use rho_harness_core::error::AppError;

    use super::common::CapturingPresenter;
    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::engine::runner::TurnRequest;
    use crate::provider::adapter::{MockAdapter, ModelStreamEvent};

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
        assert!(notices.iter().any(|n| n.contains("Context overflow detected")));
        assert!(notices.iter().any(|n| n.contains("Compacted context")));
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
        let app_config = Config {
            keep_recent_tokens: 5,
            auth_file: dir.join("auth.json"),
            ..Config::default()
        };
        mock_engine(
            model,
            MockEngineConfig {
                base_dir: dir,
                app_config,
                session_manager: None,
                built_in_tools: None,
            },
        )
    }

    #[tokio::test]
    async fn test_context_overflow_auto_recovery_halts_after_compaction() {
        let dir = std::env::temp_dir().join(format!("overflow_rec_{}", uuid::Uuid::new_v4()));
        let model = Arc::new(MockAdapter::default());
        model.add_turn(vec![Err(AppError::Provider(
            "context_length_exceeded: maximum context length is 128000 tokens".to_string(),
        ))]);
        model.add_turn(vec![Ok(ModelStreamEvent::Text("recovered from overflow".to_string()))]);

        let engine = recovery_engine(&dir, model);
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
        assert_eq!(output.status, crate::engine::runner::RunStatus::Compacted);
        assert_eq!(output.final_text, "");
        assert_recovery_presenter(&presenter);
    }

    #[tokio::test]
    async fn test_context_overflow_fails_if_overflow_persists() {
        let dir = std::env::temp_dir().join(format!("overflow_loop_{}", uuid::Uuid::new_v4()));
        let model = Arc::new(MockAdapter::default());
        model.add_turn(vec![Err(AppError::Provider("context_length_exceeded".to_string()))]);
        model.add_turn(vec![Err(AppError::Provider("context_length_exceeded".to_string()))]);

        let engine = recovery_engine(&dir, model);
        let session_id = engine.session_manager.session_id.clone();
        populate_recovery_history(&engine.session_manager, &session_id).await;

        let presenter = Arc::new(CapturingPresenter::default());
        let first = engine
            .run_turn(TurnRequest::new("Persistent overflow prompt"), presenter.clone())
            .await
            .unwrap();
        assert_eq!(first.status, crate::engine::runner::RunStatus::Compacted);

        let second = engine
            .run_turn(TurnRequest::new("Persistent overflow prompt"), presenter)
            .await;
        assert!(second.is_err());
    }
}

mod sequential {
    use super::common;
    use rho_harness_core::config::Config;
    use rho_harness_core::model::{AssistantContent, ChatMessage, ToolCall, ToolFunction, UserContent};
    use rho_harness_core::session::tree::TreeNodeKind;
    use std::sync::Arc;

    use crate::auth::AuthStore;
    use crate::engine::AgentEngine;
    use crate::engine::builder::AgentEngineBuilder;
    use crate::provider::adapter::MockAdapter;

    async fn test_engine(label: &str, model: Option<Arc<MockAdapter>>) -> AgentEngine {
        let dir = std::env::temp_dir().join(format!("sequential_{label}_{}", uuid::Uuid::new_v4()));
        let config = Config {
            sessions_dir: dir.join("sessions"),
            auth_file: dir.join("auth.json"),
            keep_recent_tokens: 10,
            ..Default::default()
        };
        let auth_store = AuthStore::load(&config.auth_file).unwrap_or_default();
        let mut builder = AgentEngineBuilder::new(config, auth_store)
            .base_dir(dir)
            .tools(Vec::new());
        if let Some(m) = model {
            builder = builder.model(m);
        }
        builder.build().await.unwrap()
    }

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

        let engine = test_engine("seq", Some(mock.clone())).await;
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
    use rho_harness_core::session::tree::TreeNodeKind;

    use super::common::CapturingPresenter;
    use crate::engine::eval::mock::{MockEngineConfig, mock_engine};
    use crate::engine::runner::TurnRequest;
    use crate::provider::adapter::MockAdapter;

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
        mock_engine(
            model,
            MockEngineConfig {
                base_dir: dir,
                app_config,
                session_manager: None,
                built_in_tools: None,
            },
        )
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
    async fn test_proactive_auto_compaction_before_turn() {
        let dir = std::env::temp_dir().join(format!("proactive_{}", uuid::Uuid::new_v4()));
        let engine = proactive_engine(&dir, 127_980);
        let session_id = engine.session_manager.session_id.clone();
        populate_proactive_history(&engine.session_manager, &session_id).await;

        let presenter = Arc::new(CapturingPresenter::default());
        let output = engine
            .run_turn(TurnRequest::new("Turn 3 request"), presenter.clone())
            .await
            .unwrap();
        assert_eq!(output.status, crate::engine::runner::RunStatus::Compacted);
        assert_eq!(output.final_text, "");
        assert_eq!(output.requests, 0);
        assert_eq!(output.tool_calls_count, 0);
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
        history: &mut Vec<rho_harness_core::model::ChatMessage>,
        tokens: u64,
    ) {
        record_test_turn_usage(engine, tokens);
        engine.check_proactive_compaction(presenter, history, 0).await.unwrap();
    }

    fn threshold_engine(dir: &std::path::Path) -> crate::engine::AgentEngine {
        let app_config = Config {
            model: "mock-model".to_string(),
            keep_recent_tokens: 5,
            auth_file: dir.join("auth.json"),
            ..Config::default()
        };
        let model = Arc::new(MockAdapter::text("response"));
        mock_engine(
            model,
            MockEngineConfig {
                base_dir: dir,
                app_config,
                session_manager: None,
                built_in_tools: None,
            },
        )
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

    #[tokio::test]
    async fn test_proactive_auto_compaction_at_reserve_threshold() {
        let dir = std::env::temp_dir().join(format!("proactive_reserve_{}", uuid::Uuid::new_v4()));
        let engine = threshold_engine(&dir);
        let mut history = seed_threshold_history(&engine).await;
        let presenter = Arc::new(CapturingPresenter::default());

        // Default reserve of 16,384 against a 128k window triggers at 111,617.
        check_compaction_step(&engine, &presenter, &mut history, 111_616).await;
        assert!(presenter.notices.lock().unwrap().is_empty());
        check_compaction_step(&engine, &presenter, &mut history, 111_617).await;
        let notices = presenter.notices.lock().unwrap().clone();
        assert!(notices.iter().any(|n| n.contains("Auto-compacted context")));
    }
}
