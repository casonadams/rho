//! Compactor tests: LLM summarization, orchestration, overflow recovery,
//! proactive auto-compaction, and sequential compaction accumulation.

mod branch {
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
}

mod compactor {
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
}

mod common {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use rho_harness_core::presentation::activity::ActivityToken;
    use rho_harness_core::presentation::presenter::Presenter;
    use rho_harness_core::presentation::stream::ToolStreamPort;
    use rho_harness_core::presentation::{SessionStatus, ToolLine, WelcomeDisplay};

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
    use rho_harness_core::config::Config;
    use rho_harness_core::session::tree::TreeNodeKind;
    use rig::agent::ModelHandle;
    use rig::memory::ConversationMemory;
    use rig::message::{
        AssistantContent, Message, Text, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
    };
    use rig::test_utils::MockCompletionModel;

    use crate::auth::AuthStore;
    use crate::engine::builder::AgentEngineBuilder;

    async fn test_engine(label: &str, model: Option<MockCompletionModel>) -> crate::engine::AgentEngine {
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
            builder = builder.model(ModelHandle::new(m));
        }
        builder.build().await.unwrap()
    }

    fn tool_turn(cid: &str, tool: &str, path: &str, user: &str, done: &str) -> Vec<Message> {
        let call = ToolCall::new(
            ToolCallId::new_or_mint(cid),
            ToolFunction::new(tool.to_string(), serde_json::json!({"path": path})),
        );
        let res = ToolResult {
            call: ToolCallId::new_or_mint(cid),
            provider: None,
            name: tool.to_string(),
            content: vec![ToolResultContent::Text(Text::new("data"))],
        };
        vec![
            Message::user(user),
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call)],
            },
            Message::User {
                content: vec![UserContent::ToolResult(res)],
            },
            Message::assistant(done),
        ]
    }

    async fn populate_test_turns(sm: &rho_harness_core::session::SessionManager, sid: &str) {
        let turn1 = tool_turn("c1", "read", "Cargo.toml", "Read config", "Done read");
        let turn2 = tool_turn("c2", "write", "src/storage.rs", "Edit storage", "Done write");
        for turn in [
            turn1,
            turn2,
            vec![Message::user("Verify"), Message::assistant("Verified")],
        ] {
            ConversationMemory::append(sm, sid, turn).await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_compact_session_with_file_tracking_and_metrics() {
        let mock =
            MockCompletionModel::text("## Goal\nRefactor session storage\n\n## Progress\n### Done\n- [x] Read files");
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
        assert!(matches!(&active_messages[0], Message::System { .. }));
    }

    #[tokio::test]
    async fn test_compact_session_empty_or_single_node() {
        let engine = test_engine("empty_session", None).await;
        let stats = engine.compact_session(None).await.unwrap();
        assert_eq!(stats.saved_tokens, 0);

        let session_id = engine.session_manager.session_id.clone();
        ConversationMemory::append(
            &engine.session_manager,
            &session_id,
            vec![Message::user("Hello"), Message::assistant("Hi")],
        )
        .await
        .unwrap();

        let stats2 = engine.compact_session(None).await.unwrap();
        assert_eq!(stats2.saved_tokens, 0);
    }

    fn split_turn_fixture() -> Vec<Message> {
        vec![
            Message::user("Preamble prompt"),
            Message::assistant("Early reply"),
            Message::user("Tool result 1"),
            Message::assistant("Mid reply with many tokens to force cut point selection"),
            Message::user("Tool result 2"),
            Message::assistant("Final suffix reply"),
        ]
    }

    fn assert_preamble_omitted(messages: &[Message]) {
        assert!(matches!(&messages[0], Message::System { .. }));
        assert!(!messages.iter().any(|m| match m {
            Message::User { content } => content.iter().any(|c| match c {
                rig::message::UserContent::Text(t) => t.text.contains("Preamble prompt"),
                _ => false,
            }),
            _ => false,
        }));
    }

    #[tokio::test]
    async fn test_compact_session_split_turn_prunes_prefix_messages() {
        let mock = MockCompletionModel::text("## Goal\nComplete huge operation");
        let engine = test_engine("split_turn", Some(mock)).await;
        let sid = engine.session_manager.session_id.clone();
        ConversationMemory::append(&engine.session_manager, &sid, split_turn_fixture())
            .await
            .unwrap();

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
            Message::user("Please build an entire large subsystem with lots of details."),
            Message::assistant("I will now write multiple files and refactor the architecture comprehensively."),
        ];
        engine.session_manager.append(sid, messages).await.unwrap();

        let stats = engine.compact_session(None).await.unwrap();
        assert!(stats.summary.len() <= 120);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

mod overflow {
    use rig::agent::StreamingError;
    use rig::completion::{CompletionError, PromptError};

    use crate::engine::compactor::{is_context_overflow_error, is_context_overflow_message};

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
        let completion_err = StreamingError::Completion(CompletionError::ResponseError(
            "prompt is too long: 210000 tokens".to_string(),
        ));
        assert!(is_context_overflow_error(&completion_err));

        let prompt_err = StreamingError::Prompt(Box::new(PromptError::CompletionError(
            CompletionError::ResponseError("context_length_exceeded".to_string()),
        )));
        assert!(is_context_overflow_error(&prompt_err));

        let unrelated = StreamingError::Completion(CompletionError::ResponseError(
            "Model overloaded, try again later".to_string(),
        ));
        assert!(!is_context_overflow_error(&unrelated));
    }
}

mod recovery {
    use std::sync::Arc;

    use rho_harness_core::config::Config;
    use rig::completion::Usage;
    use rig::memory::ConversationMemory;
    use rig::message::Message;
    use rig::test_utils::{MockCompletionModel, MockError, MockStreamEvent};

    use super::common::CapturingPresenter;
    use crate::engine::eval::mock::{MockEngineConfig, final_event, mock_engine};
    use crate::engine::runner::TurnRequest;

    async fn populate_recovery_history(sm: &rho_harness_core::session::SessionManager, sid: &str) {
        for i in 1..=2 {
            let msgs = vec![
                Message::user(format!("Old turn {i}")),
                Message::assistant(format!("Old response {i}")),
            ];
            ConversationMemory::append(sm, sid, msgs).await.unwrap();
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

    fn recovery_engine(dir: &std::path::Path, model: MockCompletionModel) -> crate::engine::AgentEngine {
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

    fn sample_overflow_usage() -> Usage {
        Usage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_context_overflow_auto_recovery_halts_after_compaction() {
        let dir = std::env::temp_dir().join(format!("overflow_rec_{}", uuid::Uuid::new_v4()));
        let model = MockCompletionModel::from_stream_turns([
            vec![MockStreamEvent::Error(MockError::provider(
                "context_length_exceeded: maximum context length is 128000 tokens",
            ))],
            vec![
                MockStreamEvent::text("recovered from overflow"),
                final_event(sample_overflow_usage()),
            ],
        ]);

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
        let model = MockCompletionModel::from_stream_turns([
            vec![MockStreamEvent::Error(MockError::provider("context_length_exceeded"))],
            vec![MockStreamEvent::Error(MockError::provider("context_length_exceeded"))],
        ]);

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
    use rho_harness_core::config::Config;
    use rho_harness_core::session::tree::TreeNodeKind;
    use rig::agent::ModelHandle;
    use rig::memory::ConversationMemory;
    use rig::message::{
        AssistantContent, Message, Text, ToolCall, ToolCallId, ToolFunction, ToolResult, ToolResultContent, UserContent,
    };
    use rig::test_utils::{MockCompletionModel, MockTurn};

    use crate::auth::AuthStore;
    use crate::engine::AgentEngine;
    use crate::engine::builder::AgentEngineBuilder;

    async fn test_engine(label: &str, model: Option<MockCompletionModel>) -> AgentEngine {
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
            builder = builder.model(ModelHandle::new(m));
        }
        builder.build().await.unwrap()
    }

    fn file_turn(call_id: &str, tool: &str, path: &str) -> Vec<Message> {
        vec![
            Message::user(format!("Execute {tool} on {path}")),
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(ToolCall::new(
                    ToolCallId::new_or_mint(call_id),
                    ToolFunction::new(tool.to_string(), serde_json::json!({"path": path})),
                ))],
            },
            Message::User {
                content: vec![UserContent::ToolResult(ToolResult {
                    call: ToolCallId::new_or_mint(call_id),
                    provider: None,
                    name: tool.to_string(),
                    content: vec![ToolResultContent::Text(Text::new("done"))],
                })],
            },
            Message::assistant(format!("Completed {tool} on {path}.")),
        ]
    }

    async fn append_file_turns(
        sm: &rho_harness_core::session::SessionManager,
        sid: &str,
        turns: &[(&str, &str, &str)],
    ) {
        for (call_id, tool, path) in turns {
            let turn = file_turn(call_id, tool, path);
            ConversationMemory::append(sm, sid, turn).await.unwrap();
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

    fn assert_no_system_messages_in_prompts(requests: &[rig::completion::CompletionRequest]) {
        assert!(requests.len() >= 2);
        for req in &requests[1..] {
            assert!(!format!("{req:?}").contains("[System]: ## Goal"));
        }
    }

    fn sequential_mock() -> MockCompletionModel {
        MockCompletionModel::new([
            MockTurn::text("## Goal\nFirst compaction"),
            MockTurn::text("## Goal\nPrefix 1"),
            MockTurn::text("## Goal\nSecond compaction"),
            MockTurn::text("## Goal\nPrefix 2"),
        ])
    }

    #[tokio::test]
    async fn test_sequential_compactions_accumulate_files() {
        let mock = sequential_mock();
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
    use std::sync::Arc;

    use rho_harness_core::config::Config;
    use rho_harness_core::session::tree::TreeNodeKind;
    use rig::completion::Usage;
    use rig::memory::ConversationMemory;
    use rig::message::Message;
    use rig::test_utils::{MockCompletionModel, MockStreamEvent};

    use super::common::CapturingPresenter;
    use crate::engine::eval::mock::{MockEngineConfig, final_event, mock_engine};
    use crate::engine::runner::TurnRequest;

    async fn populate_proactive_history(sm: &rho_harness_core::session::SessionManager, sid: &str) {
        for i in 1..=2 {
            let msgs = vec![
                Message::user(format!("Turn {i} request with many tokens")),
                Message::assistant(format!("Turn {i} response")),
            ];
            ConversationMemory::append(sm, sid, msgs).await.unwrap();
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
        let usage = Usage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            ..Default::default()
        };
        let model =
            MockCompletionModel::from_stream_turns([[MockStreamEvent::text("turn response"), final_event(usage)]]);
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
        let u = Usage {
            input_tokens: tokens,
            ..Default::default()
        }
        .into();
        engine
            .usage
            .record_turn(crate::engine::tracking::TurnUsage::new(u, u), 100);
    }

    async fn check_compaction_step(
        engine: &crate::engine::AgentEngine,
        presenter: &CapturingPresenter,
        history: &mut Vec<Message>,
        tokens: u64,
    ) {
        record_test_turn_usage(engine, tokens);
        engine
            .check_proactive_compaction(presenter, (history, 0))
            .await
            .unwrap();
    }

    fn threshold_engine(dir: &std::path::Path) -> crate::engine::AgentEngine {
        let app_config = Config {
            model: "mock-model".to_string(),
            keep_recent_tokens: 5,
            auth_file: dir.join("auth.json"),
            ..Config::default()
        };
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::text("response"),
            final_event(Usage::default()),
        ]]);
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

    async fn seed_threshold_history(engine: &crate::engine::AgentEngine) -> Vec<Message> {
        let sid = &engine.session_manager.session_id;
        let msgs = vec![Message::user("prior prompt"), Message::assistant("prior response")];
        ConversationMemory::append(&engine.session_manager, sid, msgs)
            .await
            .unwrap();
        ConversationMemory::load(&engine.session_manager, sid).await.unwrap()
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
