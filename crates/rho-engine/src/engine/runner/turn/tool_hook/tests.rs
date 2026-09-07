//! Turn tool-execution hook tests: path extraction, subtree activation,
//! result gating, steering, and runtime model switching.

mod activation {
    use super::super::{TurnToolExecutionHook, extract_path_argument};
    use crate::engine::context::ProjectContext;
    use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
    use rho_harness_core::session::SessionManager;
    use rig::agent::AgentBuilder;
    use rig::test_utils::{MockCompletionModel, MockTurn};
    use serde_json::json;
    use std::path::Path;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn mock_sink(dir: &Path) -> Arc<TerminalApprovalSink> {
        let session = SessionManager::new(dir, None).unwrap();
        TerminalApprovalSink::new(
            &crate::engine::eval::presenter::presenter(),
            TerminalSinkConfig {
                model_label: "test-model".to_string(),
                run_tracker: crate::engine::metrics::RunTracker::default(),
            },
            session,
        )
    }

    #[test]
    fn test_extract_path_argument_variants() {
        let cases = [
            (json!({"path": "src/lib.rs"}), Some("src/lib.rs")),
            (json!({"file_path": "crates/foo/bar.rs"}), Some("crates/foo/bar.rs")),
            (json!({"filePath": "nested/path.rs"}), Some("nested/path.rs")),
            (json!({"path": "  \"quoted/path.rs\"  "}), Some("quoted/path.rs")),
            (json!({"path": "  'single_quoted.rs'  "}), Some("single_quoted.rs")),
            (json!({"path": ""}), None),
            (json!({"other": 123}), None),
            (json!({}), None),
            (json!(null), None),
        ];
        for (arg, expected) in cases {
            assert_eq!(extract_path_argument(&arg), expected);
        }
    }

    fn setup_subtree_repo(repo_root: &std::path::Path) {
        let plugin_crate = repo_root.join("crates").join("rho-plugin-sdk");
        let plugin_src = plugin_crate.join("src");
        std::fs::create_dir_all(repo_root.join(".git")).unwrap();
        std::fs::create_dir_all(&plugin_src).unwrap();
        std::fs::write(repo_root.join("AGENTS.md"), "# Root Workspace Instructions\n").unwrap();
        std::fs::write(plugin_crate.join("AGENTS.md"), "# Plugin SDK Subtree Instructions\n").unwrap();
        std::fs::write(plugin_src.join("lib.rs"), "pub fn hello() {}").unwrap();
    }

    #[tokio::test]
    async fn test_tool_hook_dynamic_subtree_activation_during_turn() {
        let temp = tempfile::tempdir().unwrap();
        let repo_root = temp.path().join("repo");
        setup_subtree_repo(&repo_root);

        let initial_ctx = ProjectContext::discover(&repo_root, None).await;
        assert_eq!(initial_ctx.instruction_files.len(), 1);

        let shared_ctx = Arc::new(Mutex::new(Some((repo_root.clone(), initial_ctx))));
        let hook = TurnToolExecutionHook::new(mock_sink(&repo_root), "anthropic", None)
            .with_project_context(shared_ctx.clone());

        let model = MockCompletionModel::new([
            MockTurn::tool_call("1", "read", json!({"path": "crates/rho-plugin-sdk/src/lib.rs"})),
            MockTurn::text("file inspected"),
        ]);

        let agent = AgentBuilder::new(model)
            .tool(crate::tools::ReadTool::new(&repo_root))
            .add_hook(hook)
            .record_content_telemetry(false)
            .build();
        let response = agent.runner("Inspect plugin sdk").max_turns(3).run().await.unwrap();
        assert_eq!(response.output, "file inspected");

        let guard = shared_ctx.lock().await;
        let (_, updated_ctx) = guard.as_ref().unwrap();
        assert_eq!(updated_ctx.instruction_files.len(), 2);
        assert_eq!(updated_ctx.instruction_files[1].1, "# Plugin SDK Subtree Instructions");
    }
}

mod gating {
    use super::super::{gated_result, text_render, with_omission_note};
    use rig::agent::hook::ToolResultAction;
    use rig::completion::message::{DocumentSourceKind, Image, ImageMediaType, ToolResultContent};
    use rig::tool::ToolOutput;

    fn text_output(text: &str) -> ToolOutput {
        ToolOutput::text(text)
    }

    fn image_block(media_type: Option<ImageMediaType>) -> ToolResultContent {
        ToolResultContent::Image(Image {
            data: DocumentSourceKind::Base64("aGVsbG8=".to_string()),
            media_type,
            ..Image::default()
        })
    }

    fn image_output(media_type: Option<rig::completion::message::ImageMediaType>) -> ToolOutput {
        ToolOutput::content(vec![
            ToolResultContent::text("Read image file"),
            image_block(media_type),
        ])
        .expect("non-empty")
    }

    #[test]
    fn plain_text_renders_byte_identical() {
        let (text, has_images) = text_render(&text_output("hello world"));
        assert_eq!(text, "hello world");
        assert!(!has_images);
    }

    #[test]
    fn json_output_renders_as_json_without_images() {
        let output =
            ToolOutput::content(vec![ToolResultContent::json(serde_json::json!({"a": 1}))]).expect("non-empty");
        let (text, has_images) = text_render(&output);
        assert_eq!(text, r#"{"a":1}"#);
        assert!(!has_images);
    }

    #[test]
    fn image_blocks_render_as_placeholders() {
        let (text, has_images) = text_render(&image_output(Some(rig::completion::message::ImageMediaType::PNG)));
        assert_eq!(text, "Read image file\n[image: image/png]");
        assert!(has_images);
    }

    #[test]
    fn image_without_media_type_renders_unknown_placeholder() {
        let (text, _) = text_render(&image_output(None));
        assert_eq!(text, "Read image file\n[image: unknown]");
    }

    #[test]
    fn capable_providers_keep_image_results() {
        for provider in ["anthropic", "gemini", "chatgpt"] {
            let (action, display) = gated_result(
                &image_output(Some(rig::completion::message::ImageMediaType::PNG)),
                provider,
            );
            assert_eq!(action, ToolResultAction::keep());
            assert_eq!(display, "Read image file\n[image: image/png]");
        }
    }

    #[test]
    fn incapable_providers_get_image_results_rewritten_with_note() {
        let (action, display) = gated_result(
            &image_output(Some(rig::completion::message::ImageMediaType::JPEG)),
            "openai",
        );
        let expected = "Read image file\n[image: image/jpeg]\n[Image in tool result omitted: openai does not support images in tool results.]";
        assert_eq!(action, ToolResultAction::rewrite(expected));
        assert_eq!(display, expected);
    }

    #[test]
    fn unknown_providers_get_image_results_rewritten() {
        let (action, _) = gated_result(&image_output(None), "my-custom-provider");
        assert_ne!(action, ToolResultAction::keep());
    }

    #[test]
    fn text_results_pass_through_for_every_provider() {
        for provider in ["anthropic", "openai", "unknown"] {
            let (action, display) = gated_result(&text_output("plain text"), provider);
            assert_eq!(action, ToolResultAction::keep());
            assert_eq!(display, "plain text");
        }
    }

    #[test]
    fn omission_note_stands_alone_when_no_text_parts() {
        assert_eq!(
            with_omission_note("", "openai"),
            "[Image in tool result omitted: openai does not support images in tool results.]"
        );
    }
}

mod model_switch {
    use std::sync::Arc;

    use rig::agent::hook::{AgentHook, HookContext};
    use rig::agent::{AgentBuilder, ModelHandle};
    use rig::test_utils::{MockCompletionModel, MockTurn};
    use serde_json::json;

    use super::super::TurnToolExecutionHook;
    use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
    use crate::engine::runner::turn::types::{ActiveModelSwitch, SharedModelSwitch};
    use rho_harness_core::session::SessionManager;

    fn mock_sink() -> Arc<TerminalApprovalSink> {
        let temp_dir = std::env::temp_dir().join(format!("sink_test_{}", uuid::Uuid::new_v4()));
        let session = SessionManager::new(&temp_dir, None).unwrap();
        TerminalApprovalSink::new(
            &crate::engine::eval::presenter::presenter(),
            TerminalSinkConfig {
                model_label: "test-model".to_string(),
                run_tracker: crate::engine::metrics::RunTracker::default(),
            },
            session,
        )
    }

    #[tokio::test]
    async fn test_shared_model_switch_state_transitions() {
        let switcher = SharedModelSwitch::new();
        assert!(switcher.get_handle().is_none());
        assert!(switcher.current_model().is_none());
        assert!(switcher.current_provider().is_none());
        assert!(switcher.take_switched().is_none());

        let mock = MockCompletionModel::text("test");
        let handle = ModelHandle::new(mock);
        switcher.switch_to(ActiveModelSwitch::new("gemini-2.5-pro", "gemini", handle));

        assert_eq!(switcher.current_model().as_deref(), Some("gemini-2.5-pro"));
        assert_eq!(switcher.current_provider().as_deref(), Some("gemini"));
        assert!(switcher.get_handle().is_some());
        assert_eq!(
            switcher.take_switched(),
            Some(("gemini-2.5-pro".to_string(), "gemini".to_string()))
        );
    }

    struct SwitchHook {
        switcher: Arc<SharedModelSwitch>,
        next_model: ModelHandle,
    }

    impl AgentHook for SwitchHook {
        async fn on_tool_result(
            &self,
            _ctx: &HookContext,
            _event: rig::agent::hook::ToolResultEvent<'_>,
        ) -> rig::agent::hook::ToolResultAction {
            self.switcher
                .switch_to(ActiveModelSwitch::new("model-2", "mock", self.next_model.clone()));
            rig::agent::hook::ToolResultAction::keep()
        }
    }

    #[tokio::test]
    async fn test_runtime_model_switching_across_turns_within_agent() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("out.txt");
        let switcher = Arc::new(SharedModelSwitch::new());
        let hook = TurnToolExecutionHook::new(mock_sink(), "anthropic", None).with_model_switch(Some(switcher.clone()));

        let model_1 = MockCompletionModel::new([MockTurn::tool_call(
            "1",
            "write",
            json!({"path": file, "content": "first model wrote"}),
        )]);
        let model_2 = MockCompletionModel::new([MockTurn::text("second model finished")]);
        let handle_2 = ModelHandle::new(model_2.clone());

        let switch_hook = SwitchHook {
            switcher,
            next_model: handle_2,
        };
        let agent = AgentBuilder::new(model_1.clone())
            .tool(crate::tools::WriteTool::new(dir.path()))
            .add_hook(hook)
            .add_hook(switch_hook)
            .record_content_telemetry(false)
            .build();

        let response = agent.runner("execute task").max_turns(3).run().await.unwrap();
        assert_eq!(response.output, "second model finished");
        assert_eq!((model_1.requests().len(), model_2.requests().len()), (1, 1));
    }
}

mod steering {
    use super::super::TurnToolExecutionHook;
    use super::super::steering::{
        STEERING_SKIP_REASON, attach_steering_to_output, format_steering_message, format_steering_messages,
    };
    use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
    use crate::engine::runner::turn::types::SteeringQueueProvider;
    use async_trait::async_trait;
    use rho_harness_core::session::SessionManager;
    use rig::agent::AgentBuilder;
    use rig::completion::message::{AssistantContent, ToolCall, ToolFunction};
    use rig::test_utils::{MockCompletionModel, MockTurn};
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MockSteeringQueue {
        messages: Mutex<Vec<String>>,
    }

    impl MockSteeringQueue {
        fn new(messages: &[&str]) -> Self {
            Self {
                messages: Mutex::new(messages.iter().map(|s| (*s).to_string()).collect()),
            }
        }

        fn enqueue(&self, msg: &str) {
            self.messages.lock().unwrap().push(msg.to_string());
        }
    }

    #[async_trait]
    impl SteeringQueueProvider for MockSteeringQueue {
        async fn poll_steering(&self) -> Vec<String> {
            let mut guard = self.messages.lock().unwrap();
            std::mem::take(&mut *guard)
        }
    }

    fn mock_sink() -> Arc<TerminalApprovalSink> {
        let temp_dir = std::env::temp_dir().join(format!("sink_test_{}", uuid::Uuid::new_v4()));
        let session = SessionManager::new(&temp_dir, None).unwrap();
        TerminalApprovalSink::new(
            &crate::engine::eval::presenter::presenter(),
            TerminalSinkConfig {
                model_label: "test-model".to_string(),
                run_tracker: crate::engine::metrics::RunTracker::default(),
            },
            session,
        )
    }

    fn batched_tool_calls(calls: Vec<(&str, &str, serde_json::Value)>) -> MockTurn {
        let contents = calls.into_iter().map(|(id, name, args)| {
            AssistantContent::ToolCall(ToolCall::from_wire(id, ToolFunction::new(name.to_string(), args)))
        });
        MockTurn::from_contents(contents)
    }

    #[test]
    fn test_steering_format_messages() {
        let msg = format_steering_message("stop editing");
        assert!(msg.starts_with("[USER STEERING INTERRUPT]:\nstop editing"));
        assert!(msg.contains("Please adjust your approach immediately"));

        let combined = format_steering_messages(&["one".to_string(), "two".to_string()]);
        assert!(combined.contains("one\n\ntwo"));
    }

    #[test]
    fn test_steering_attach_to_output() {
        let msg = format_steering_message("stop editing");
        let attached = attach_steering_to_output("tool output", &msg);
        assert_eq!(attached, format!("tool output\n\n{msg}"));

        let attached_empty = attach_steering_to_output("", &msg);
        assert_eq!(attached_empty, msg);
    }

    fn assert_steering_applied(model: &MockCompletionModel, file_b: &std::path::Path) {
        assert!(!file_b.exists() && model.requests().len() >= 2);
        let history = format!("{:?}", model.requests()[1].chat_history);
        for pattern in [
            "[USER STEERING INTERRUPT]",
            "pivot to another task",
            STEERING_SKIP_REASON,
        ] {
            assert!(history.contains(pattern));
        }
    }

    #[tokio::test]
    async fn test_steering_during_tool_execution_augments_result_and_skips_next() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        tokio::fs::write(&file_a, "file a content").await.unwrap();

        let steering = Arc::new(MockSteeringQueue::default());
        let hook = TurnToolExecutionHook::new(mock_sink(), "anthropic", Some(steering.clone()));
        steering.enqueue("pivot to another task");

        let model = MockCompletionModel::new([
            batched_tool_calls(vec![
                ("1", "read", json!({"path": file_a})),
                ("2", "write", json!({"path": file_b, "content": "hello"})),
            ]),
            MockTurn::text("acknowledged steering"),
        ]);

        let agent = AgentBuilder::new(model.clone())
            .tool(crate::tools::ReadTool::new(dir.path()))
            .tool(crate::tools::WriteTool::new(dir.path()))
            .add_hook(hook)
            .record_content_telemetry(false)
            .build();

        let response = agent.runner("start").max_turns(5).run().await.unwrap();
        assert_eq!(response.output, "acknowledged steering");
        assert_steering_applied(&model, &file_b);
    }

    #[tokio::test]
    async fn test_steering_before_tool_call_skips_immediately() {
        let dir = tempfile::tempdir().unwrap();
        let file_b = dir.path().join("b.txt");

        let steering = Arc::new(MockSteeringQueue::new(&["abort initial tool"]));
        let hook = TurnToolExecutionHook::new(mock_sink(), "anthropic", Some(steering));

        let model = MockCompletionModel::new([
            MockTurn::tool_call("1", "write", json!({"path": file_b, "content": "hello"})),
            MockTurn::text("tool skipped"),
        ]);

        let agent = AgentBuilder::new(model.clone())
            .tool(crate::tools::WriteTool::new(dir.path()))
            .add_hook(hook)
            .record_content_telemetry(false)
            .build();

        let response = agent.runner("start").max_turns(3).run().await.unwrap();
        assert_eq!(response.output, "tool skipped");
        assert!(!file_b.exists());

        let requests = model.requests();
        let history_str = format!("{:?}", requests[1].chat_history);
        assert!(history_str.contains(STEERING_SKIP_REASON));
        assert!(history_str.contains("abort initial tool"));
    }

    #[tokio::test]
    async fn test_no_steering_allows_all_tools_to_run() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        tokio::fs::write(&file_a, "file a content").await.unwrap();

        let steering = Arc::new(MockSteeringQueue::default());
        let hook = TurnToolExecutionHook::new(mock_sink(), "anthropic", Some(steering));

        let model = MockCompletionModel::new([
            batched_tool_calls(vec![
                ("1", "read", json!({"path": file_a})),
                ("2", "write", json!({"path": file_b, "content": "created"})),
            ]),
            MockTurn::text("all tools done"),
        ]);

        let agent = AgentBuilder::new(model.clone())
            .tool(crate::tools::ReadTool::new(dir.path()))
            .tool(crate::tools::WriteTool::new(dir.path()))
            .add_hook(hook)
            .record_content_telemetry(false)
            .build();

        let response = agent.runner("start").max_turns(5).run().await.unwrap();
        assert_eq!(response.output, "all tools done");
        assert!(file_b.exists());
        let content = tokio::fs::read_to_string(&file_b).await.unwrap();
        assert_eq!(content, "created");
    }
}
