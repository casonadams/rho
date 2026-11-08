use super::*;
use crate::engine::compactor::llm::ModelHandle;
use crate::engine::runner::turn::types::CancellationSignal;
use crate::permission::PermissionHook;
use crate::permission::policy::{build_policy, parse_scope_from_str};
use crate::permission::tests::mock::MockHookPresenter;
use crate::provider::adapter::MockAdapter;
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelCompletionResponse, ModelStreamEvent};
use crate::tools::{GenericTool, ToolResult};
use rho_harness_core::model::{AssistantContent, ChatMessage, ToolCall, ToolFunction};
use rho_harness_core::presentation::{ToolStreamPort, ToolStreamSink};
use serde_json::json;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
struct TestModel {
    answer: String,
}

#[async_trait::async_trait]
impl ModelAdapter for TestModel {
    fn model_name(&self) -> &str {
        "test-subagent-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }

    async fn complete(&self, _request: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError> {
        Ok(ModelCompletionResponse {
            content: self.answer.clone(),
            tool_calls: Vec::new(),
            usage: Default::default(),
            finish_reason: None,
        })
    }

    async fn stream(
        &self,
        _request: ModelCompletionRequest,
    ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError>
    {
        let events = vec![
            Ok(ModelStreamEvent::Text(self.answer.clone())),
            Ok(ModelStreamEvent::Usage(
                crate::engine::metrics::StructuralUsage::default(),
            )),
        ];
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

#[tokio::test]
async fn test_subagent_tool_rejects_bad_requests() {
    let tool = SubagentTool::new(None, Vec::new());
    let cases = [
        (json!({ "prompt": "search codebase" }), "No model handle configured"),
        (json!({ "prompt": 12345 }), "failed to parse subagent arguments"),
    ];
    for (args, expected) in cases {
        let result = tool.execute(args).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains(expected), "{expected}");
    }
}

#[tokio::test]
async fn test_subagent_tool_success() {
    let cases = [
        (
            "Scout completed: identified 4 services",
            json!({
                "role": "scout",
                "prompt": "Find all service definitions",
                "context_slice": "Check crates/rho-engine"
            }),
        ),
        (
            "Scout report: zero defects found",
            json!({
                "role": "critic",
                "prompt": "verify invariant AC-001",
                "context_slice": "spec details"
            }),
        ),
    ];
    for (answer, args) in cases {
        let model = Arc::new(TestModel { answer: answer.into() });
        let tool: Arc<dyn EngineTool> = Arc::new(SubagentTool::new(Some(model), Vec::new()));
        let result = tool.execute(args).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.starts_with(answer));
        assert!(result.content.contains("[subagent: 1 turns, 0 tool calls, 0 tokens]"));
    }
}

#[test]
fn test_subagent_tool_metadata() {
    let tool = SubagentTool::new(None, Vec::new());
    assert_eq!(tool.name(), "subagent");
    assert!(!tool.description().is_empty());
    let params = tool.parameters();
    assert!(params.is_object());
}

#[test]
fn test_role_tool_filtering() {
    fn dummy_tool(name: &'static str) -> Arc<dyn EngineTool> {
        Arc::new(crate::tools::GenericTool::new(name, "desc", json!({}), |_| async {
            Ok(crate::tools::ToolResult::success("ok"))
        }))
    }

    let all_tools = vec![
        dummy_tool("read"),
        dummy_tool("write"),
        dummy_tool("edit"),
        dummy_tool("bash"),
        dummy_tool("fd"),
        dummy_tool("rg"),
        dummy_tool("web_fetch"),
        dummy_tool("web_search"),
    ];

    let names = |role: SubagentRole| -> Vec<String> {
        runner::filter_tools_for_role(&all_tools, role)
            .iter()
            .map(|t| t.name().to_string())
            .collect()
    };

    let scout = names(SubagentRole::Scout);
    assert_eq!(scout, vec!["fd", "read", "rg", "web_fetch", "web_search"]);
    assert!(!scout.iter().any(|n| n == "write" || n == "bash"));

    assert_eq!(names(SubagentRole::Critic), vec!["fd", "read", "rg"]);
    assert_eq!(names(SubagentRole::Planner), vec!["fd", "read", "rg"]);
    assert_eq!(
        names(SubagentRole::General),
        vec!["bash", "edit", "fd", "read", "rg", "web_fetch", "web_search", "write"]
    );
}

#[tokio::test]
async fn test_subagent_custom_agent_execution() {
    use rho_harness_core::agents::AgentMetadata;

    let model = Arc::new(TestModel {
        answer: "Code reviewer: Clean code, no issues found".into(),
    });
    let custom_agent = AgentMetadata {
        name: "code-reviewer".to_string(),
        description: "Reviews code diffs".to_string(),
        location: ".claude/agents/code-reviewer.md".to_string(),
        tools: Some(vec!["read".to_string(), "fd".to_string()]),
        disallowed_tools: Some(vec!["bash".to_string()]),
        model: Some("sonnet".to_string()),
        max_turns: Some(8),
        system_prompt: "You are a code reviewer.".to_string(),
    };

    let subagent = Arc::new(SubagentTool::new(Some(model.clone()), Vec::new()).with_agents(vec![custom_agent]));

    assert!(subagent.description().contains("code-reviewer"));

    // 1. Invoking via explicit "agent" parameter
    let args_agent = json!({
        "agent": "code-reviewer",
        "prompt": "Review PR 42"
    });
    let res = subagent.execute(args_agent).await.unwrap();
    assert!(!res.is_error);
    assert!(res.content.starts_with("Code reviewer: Clean code, no issues found"));

    // 2. Invoking via "role" matching custom agent name
    let args_role = json!({
        "role": "code-reviewer",
        "prompt": "Review PR 42"
    });
    let res = subagent.execute(args_role).await.unwrap();
    assert!(!res.is_error);
    assert!(res.content.starts_with("Code reviewer: Clean code, no issues found"));
}

#[tokio::test]
async fn test_subagent_runner_budget_exhaustion_scenarios() {
    let ok_runner = SubagentRunner::new(
        Arc::new(TestModel {
            answer: "Synthesized findings after turn exhaustion.".into(),
        }),
        SubagentRole::Scout,
        &[],
        0,
    );
    let ok_res = ok_runner.run("search", None).await;
    assert!(
        ok_res
            .content
            .starts_with("Synthesized findings after turn exhaustion.")
    );

    let empty_runner = SubagentRunner::new(
        Arc::new(TestModel { answer: String::new() }),
        SubagentRole::Scout,
        &[],
        0,
    );
    let err_res = empty_runner.run("search", None).await;
    assert!(err_res.is_error);
    assert!(err_res.content.contains("without producing output"));
}

#[test]
fn test_prepare_history_for_recovery() {
    let tool_call = ToolCall::new(
        "call_1",
        ToolFunction::new("read", serde_json::json!({"path": "foo.rs"})),
    );
    let dangling_msg = ChatMessage::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(tool_call)],
    };
    let valid_msg = ChatMessage::Assistant {
        id: None,
        content: vec![AssistantContent::text("Here are my thoughts")],
    };

    let history = vec![valid_msg.clone(), dangling_msg];
    let sanitized = recovery::prepare_history_for_recovery(&history);
    assert_eq!(sanitized.len(), 1);
    assert_eq!(sanitized[0], valid_msg);
}

fn tool_call_turn(calls: &[(&str, &str)]) -> Vec<ModelStreamEvent> {
    calls
        .iter()
        .enumerate()
        .map(|(i, (name, command))| {
            ModelStreamEvent::ToolCall(ToolCall::new(
                format!("call-{i}"),
                ToolFunction::new(*name, json!({ "command": command })),
            ))
        })
        .collect()
}

fn text_turn(text: &str) -> Vec<ModelStreamEvent> {
    vec![ModelStreamEvent::Text(text.to_string())]
}

fn counting_tool(name: &'static str, calls: Arc<AtomicUsize>, output: String) -> Arc<dyn EngineTool> {
    Arc::new(GenericTool::new(name, "counts calls", json!({}), move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        let output = output.clone();
        async move { Ok(ToolResult::success(output)) }
    }))
}

fn tool_outputs(request: &ModelCompletionRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .filter_map(|m| match m {
            ChatMessage::User { content } => Some(content),
            _ => None,
        })
        .flatten()
        .filter_map(|c| match c {
            rho_harness_core::model::UserContent::ToolResult(tr) => Some(tr),
            _ => None,
        })
        .flat_map(|tr| tr.content.iter())
        .filter_map(|c| match c {
            rho_harness_core::model::ToolResultContent::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect()
}

fn deny_rm_context() -> TurnToolContext {
    let scope = parse_scope_from_str("[permission.bash]\n\"rm *\" = \"deny\"\n\"echo *\" = \"allow\"\n").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let hook = PermissionHook::with_policy(
        Some(dir.path().to_path_buf()),
        Arc::new(MockHookPresenter::new(false, None)),
        build_policy(None, Some(scope)),
    );
    TurnToolContext {
        permission: Some(Arc::new(hook)),
        ..Default::default()
    }
}

#[tokio::test]
async fn subagent_tool_calls_pass_through_the_permission_gate() {
    let adapter = MockAdapter::from_turns([tool_call_turn(&[("bash", "rm x")]), text_turn("done")]);
    let runs = Arc::new(AtomicUsize::new(0));
    let tools = vec![counting_tool("bash", runs.clone(), "ran".into())];
    let runner = SubagentRunner::new(Arc::new(adapter.clone()), SubagentRole::General, &tools, 5)
        .with_context(deny_rm_context());

    let result = runner.run("clean up", None).await;

    assert!(!result.is_error);
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    let outputs = tool_outputs(&adapter.requests()[1]);
    assert_eq!(outputs.len(), 1);
    assert!(outputs[0].starts_with("Permission denied"), "{outputs:?}");
}

#[tokio::test]
async fn bound_subagent_tool_enforces_the_turn_permission_policy() {
    let adapter = MockAdapter::from_turns([
        tool_call_turn(&[("bash", "rm x"), ("bash", "echo ok")]),
        text_turn("done"),
    ]);
    let runs = Arc::new(AtomicUsize::new(0));
    let tools = vec![counting_tool("bash", runs.clone(), "ran".into())];
    let unbound = SubagentTool::new(Some(Arc::new(adapter.clone())), tools);

    let bound = unbound.bind_turn(&deny_rm_context()).unwrap();
    let result = bound
        .execute(json!({ "role": "general", "prompt": "do both" }))
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("2 tool calls"));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    let outputs = tool_outputs(&adapter.requests()[1]);
    assert!(outputs[0].starts_with("Permission denied"), "{outputs:?}");
    assert_eq!(outputs[1], "ran");
}

#[tokio::test]
async fn cancelling_stops_the_subagent_during_a_tool_call() {
    let cancel = CancellationSignal::default();
    let hang_cancel = cancel.clone();
    let hanging: Arc<dyn EngineTool> = Arc::new(GenericTool::new("bash", "hangs", json!({}), move |_| {
        let cancel = hang_cancel.clone();
        async move {
            cancel.cancel();
            std::future::pending::<Result<ToolResult, AppError>>().await
        }
    }));
    let adapter = MockAdapter::from_turns([tool_call_turn(&[("bash", "sleep 999")]), text_turn("unreachable")]);
    let runner = SubagentRunner::new(Arc::new(adapter.clone()), SubagentRole::General, &[hanging], 5).with_context(
        TurnToolContext {
            cancellation: Some(cancel),
            ..Default::default()
        },
    );

    let result = runner.run("hang", None).await;

    assert!(result.is_error);
    assert!(result.content.contains("interrupted"));
    assert_eq!(adapter.requests().len(), 1);
}

#[tokio::test]
async fn later_turns_keep_the_task_and_truncate_large_tool_output() {
    let adapter = MockAdapter::from_turns([tool_call_turn(&[("read", "big")]), text_turn("done")]);
    let tools = vec![counting_tool("read", Arc::new(AtomicUsize::new(0)), "a".repeat(30_000))];
    let runner = SubagentRunner::new(Arc::new(adapter.clone()), SubagentRole::Scout, &tools, 5);

    runner.run("find x", Some("ctx")).await;

    let second = &adapter.requests()[1];
    assert_eq!(second.messages[0], ChatMessage::user("Context:\nctx\n\nTask:\nfind x"));
    let outputs = tool_outputs(second);
    assert!(outputs[0].ends_with("[truncated 10000 characters]"));
}

#[tokio::test]
async fn model_failure_mid_run_returns_gathered_findings() {
    let adapter = MockAdapter::from_result_turns([
        text_turn("found the module")
            .into_iter()
            .chain(tool_call_turn(&[("read", "x")]))
            .map(Ok)
            .collect(),
        vec![Err(AppError::Other(anyhow::anyhow!("boom")))],
    ]);
    let tools = vec![counting_tool("read", Arc::new(AtomicUsize::new(0)), "body".into())];
    let runner = SubagentRunner::new(Arc::new(adapter), SubagentRole::Scout, &tools, 5);

    let result = runner.run("look", None).await;

    assert!(!result.is_error);
    assert!(result.content.contains("stopped after a model call failed"));
    assert!(result.content.contains("found the module"));
}

#[tokio::test]
async fn model_failure_before_any_findings_is_an_error() {
    let adapter = MockAdapter::from_result_turns([vec![Err(AppError::Other(anyhow::anyhow!("boom")))]]);
    let runner = SubagentRunner::new(Arc::new(adapter), SubagentRole::Scout, &[], 5);

    let result = runner.run("look", None).await;

    assert!(result.is_error);
    assert!(result.content.contains("boom"));
}

#[tokio::test]
async fn unknown_tool_call_is_reported_back_to_the_model() {
    let adapter = MockAdapter::from_turns([tool_call_turn(&[("nope", "x")]), text_turn("done")]);
    let runner = SubagentRunner::new(Arc::new(adapter.clone()), SubagentRole::Scout, &[], 5);

    runner.run("look", None).await;

    assert_eq!(tool_outputs(&adapter.requests()[1]), vec!["Error: Unknown tool 'nope'"]);
}

#[derive(Default)]
struct CollectedProgress(Mutex<Vec<String>>);

impl ToolStreamSink for CollectedProgress {
    fn tool_chunk(&self, chunk: String) {
        self.0.lock().unwrap().push(chunk);
    }
}

#[tokio::test]
async fn subagent_reports_turn_progress() {
    let adapter = MockAdapter::from_turns([tool_call_turn(&[("read", "x"), ("rg", "y")]), text_turn("done")]);
    let tools = vec![
        counting_tool("read", Arc::new(AtomicUsize::new(0)), "a".into()),
        counting_tool("rg", Arc::new(AtomicUsize::new(0)), "b".into()),
    ];
    let progress = Arc::new(CollectedProgress::default());
    let runner =
        SubagentRunner::new(Arc::new(adapter.clone()), SubagentRole::Scout, &tools, 5).with_context(TurnToolContext {
            progress: ToolStreamPort::new(Some(progress.clone())),
            ..Default::default()
        });

    runner.run("look", None).await;

    assert_eq!(*progress.0.lock().unwrap(), vec!["subagent turn 1/5: read, rg\n"]);
    assert_eq!(tool_outputs(&adapter.requests()[1]), vec!["a", "b"]);
}

#[tokio::test]
async fn unknown_agent_or_role_names_are_rejected_with_valid_options() {
    use rho_harness_core::agents::AgentMetadata;

    let agent = AgentMetadata {
        name: "code-reviewer".to_string(),
        description: String::new(),
        location: String::new(),
        tools: None,
        disallowed_tools: None,
        model: None,
        max_turns: None,
        system_prompt: "review".to_string(),
    };
    let tool = SubagentTool::new(Some(Arc::new(MockAdapter::text("x"))), Vec::new()).with_agents(vec![agent]);

    for field in ["role", "agent"] {
        let result = tool.execute(json!({ field: "scuot", "prompt": "go" })).await.unwrap();
        assert!(result.is_error, "{field}");
        assert!(result.content.contains("unknown subagent 'scuot'"), "{field}");
        assert!(result.content.contains("code-reviewer"), "{field}");
    }
}

fn agent(name: &str, model: Option<&str>, tools: Option<&[&str]>) -> rho_harness_core::agents::AgentMetadata {
    rho_harness_core::agents::AgentMetadata {
        name: name.to_string(),
        description: String::new(),
        location: String::new(),
        tools: tools.map(|t| t.iter().map(ToString::to_string).collect()),
        disallowed_tools: None,
        model: model.map(ToString::to_string),
        max_turns: None,
        system_prompt: format!("You are {name}."),
    }
}

fn named_tool(name: &'static str) -> Arc<dyn EngineTool> {
    counting_tool(name, Arc::new(AtomicUsize::new(0)), "ok".into())
}

#[tokio::test]
async fn custom_agent_model_is_resolved_and_used() {
    let default = MockAdapter::text("from default");
    let special = MockAdapter::text("from special");
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let resolver_seen = seen.clone();
    let special_handle: ModelHandle = Arc::new(special);
    let resolver: SubagentModelResolver = Arc::new(move |name| {
        resolver_seen.lock().unwrap().push(name.to_string());
        Ok(special_handle.clone())
    });
    let tool = SubagentTool::new(Some(Arc::new(default)), Vec::new())
        .with_agents(vec![agent("reviewer", Some("openai/gpt-x"), None)])
        .with_model_resolver(resolver);

    let result = tool
        .execute(json!({ "agent": "reviewer", "prompt": "go" }))
        .await
        .unwrap();

    assert!(result.content.starts_with("from special"), "{}", result.content);
    assert_eq!(*seen.lock().unwrap(), vec!["openai/gpt-x"]);
}

#[tokio::test]
async fn inherit_aliases_and_missing_model_use_the_parent_model() {
    for model in [
        None,
        Some("inherit"),
        Some("Sonnet"),
        Some("opus"),
        Some("haiku"),
        Some("  "),
    ] {
        let resolver: SubagentModelResolver = Arc::new(|_| Err("must not be called".into()));
        let tool = SubagentTool::new(Some(Arc::new(MockAdapter::text("from default"))), Vec::new())
            .with_agents(vec![agent("reviewer", model, None)])
            .with_model_resolver(resolver);

        let result = tool
            .execute(json!({ "agent": "reviewer", "prompt": "go" }))
            .await
            .unwrap();

        assert!(
            result.content.starts_with("from default"),
            "{model:?}: {}",
            result.content
        );
    }
}

#[tokio::test]
async fn unresolvable_agent_model_is_an_error_not_a_silent_fallback() {
    let failing: SubagentModelResolver = Arc::new(|_| Err("no credentials".into()));
    let cases = [
        (
            SubagentTool::new(Some(Arc::new(MockAdapter::text("x"))), Vec::new())
                .with_agents(vec![agent("reviewer", Some("acme/m1"), None)])
                .with_model_resolver(failing),
            "failed to resolve model 'acme/m1' for subagent: no credentials",
        ),
        (
            SubagentTool::new(Some(Arc::new(MockAdapter::text("x"))), Vec::new()).with_agents(vec![agent(
                "reviewer",
                Some("acme/m1"),
                None,
            )]),
            "no model resolver is configured",
        ),
    ];
    for (tool, expected) in cases {
        let result = tool
            .execute(json!({ "agent": "reviewer", "prompt": "go" }))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.content.contains(expected), "{}", result.content);
    }
}

#[test]
fn subagent_concurrency_safety_follows_the_tool_set() {
    let tools = vec![
        named_tool("read"),
        named_tool("rg"),
        named_tool("bash"),
        named_tool("write"),
    ];
    let tool = SubagentTool::new(None, tools).with_agents(vec![
        agent("reader", None, Some(&["read", "rg"])),
        agent("writer", None, Some(&["read", "write"])),
        agent("everything", None, None),
        agent("toolless", None, Some(&[])),
    ]);
    let cases = [
        (json!({ "prompt": "p" }), true),
        (json!({ "role": "critic", "prompt": "p" }), true),
        (json!({ "role": "general", "prompt": "p" }), false),
        (json!({ "agent": "reader", "prompt": "p" }), true),
        (json!({ "agent": "writer", "prompt": "p" }), false),
        (json!({ "agent": "everything", "prompt": "p" }), false),
        (json!({ "agent": "toolless", "prompt": "p" }), false),
        (json!({ "agent": "missing", "prompt": "p" }), false),
        (json!({ "prompt": 7 }), false),
    ];
    for (args, expected) in cases {
        assert_eq!(tool.concurrency_safe(&args), expected, "{args}");
    }
}

fn parent_history_context() -> TurnToolContext {
    let history = vec![
        ChatMessage::System {
            content: "hidden".into(),
        },
        ChatMessage::user("fix the parser"),
        ChatMessage::Assistant {
            id: None,
            content: vec![
                AssistantContent::text("looking at it"),
                AssistantContent::ToolCall(ToolCall::new("c", ToolFunction::new("read", json!({})))),
            ],
        },
        ChatMessage::user("also check the lexer"),
    ];
    TurnToolContext {
        parent_history: history.into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn inherit_context_shares_a_text_only_parent_transcript() {
    let cases = [
        (json!({ "prompt": "go", "inherit_context": true }), true),
        (json!({ "prompt": "go" }), false),
        (json!({ "prompt": "go", "inherit_context": false }), false),
    ];
    for (args, shared) in cases {
        let adapter = MockAdapter::text("done");
        let tool = SubagentTool::new(Some(Arc::new(adapter.clone())), Vec::new());
        let bound = tool.bind_turn(&parent_history_context()).unwrap();

        bound.execute(args.clone()).await.unwrap();

        let prompt = adapter.requests()[0].prompt.clone();
        assert_eq!(prompt.contains("Parent conversation"), shared, "{args}");
        if shared {
            assert!(prompt.contains("User: fix the parser"));
            assert!(prompt.contains("Assistant: looking at it"));
            assert!(prompt.contains("User: also check the lexer"));
            assert!(!prompt.contains("hidden"));
            assert!(prompt.ends_with("Task:\ngo"));
        }
    }
}

#[test]
fn parent_transcript_keeps_only_recent_messages_and_truncates_long_ones() {
    let mut history: Vec<ChatMessage> = (0..30).map(|i| ChatMessage::user(format!("message {i}"))).collect();
    history.push(ChatMessage::user("x".repeat(5_000)));

    let rendered = runner::render_parent_conversation(&history);

    assert!(!rendered.contains("message 9\n"));
    assert!(rendered.contains("message 11"));
    assert_eq!(rendered.matches("User: ").count(), 20);
    assert!(rendered.chars().filter(|c| *c == 'x').count() == 1_500);
    assert_eq!(runner::render_parent_conversation(&[]), "");
}

#[tokio::test]
async fn inherit_context_and_slice_are_combined() {
    let adapter = MockAdapter::text("done");
    let bound = SubagentTool::new(Some(Arc::new(adapter.clone())), Vec::new())
        .bind_turn(&parent_history_context())
        .unwrap();

    bound
        .execute(json!({ "prompt": "go", "inherit_context": true, "context_slice": "extra notes" }))
        .await
        .unwrap();

    let prompt = adapter.requests()[0].prompt.clone();
    assert!(prompt.contains("User: fix the parser"));
    assert!(prompt.contains("extra notes"));
}

#[tokio::test]
async fn custom_agent_progress_is_labelled_with_its_name() {
    let adapter = MockAdapter::from_turns([tool_call_turn(&[("read", "x")]), text_turn("done")]);
    let progress = Arc::new(CollectedProgress::default());
    let ctx = TurnToolContext {
        progress: ToolStreamPort::new(Some(progress.clone())),
        ..Default::default()
    };
    let tool = SubagentTool::new(Some(Arc::new(adapter)), vec![named_tool("read")])
        .with_agents(vec![agent("reviewer", None, Some(&["read"]))])
        .bind_turn(&ctx)
        .unwrap();

    tool.execute(json!({ "agent": "reviewer", "prompt": "go" }))
        .await
        .unwrap();

    assert_eq!(*progress.0.lock().unwrap(), vec!["subagent reviewer turn 1/10: read\n"]);
}

#[test]
fn long_parent_message_keeps_both_its_opening_and_its_closing_text() {
    let long = format!("OPENING {} codeword is kumquat CLOSING", "filler ".repeat(1_500));
    let history = vec![ChatMessage::user(long)];

    let rendered = runner::render_parent_conversation(&history);

    assert!(rendered.contains("OPENING"));
    assert!(rendered.contains("kumquat CLOSING"));
    assert!(rendered.contains("characters omitted"));
}
