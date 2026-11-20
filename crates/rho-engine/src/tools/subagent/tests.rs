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
        (json!({ "prompt": 12345 }), "Invalid arguments for subagent"),
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
        assert!(
            result
                .content
                .contains("[subagent: 1 turns, 0 tool calls, 0 tokens, test-provider/test-subagent-model]")
        );
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

#[test]
fn custom_agents_never_get_the_subagent_tool() {
    use rho_harness_core::agents::AgentMetadata;
    fn dummy_tool(name: &'static str) -> Arc<dyn EngineTool> {
        Arc::new(crate::tools::GenericTool::new(name, "desc", json!({}), |_| async {
            Ok(crate::tools::ToolResult::success("ok"))
        }))
    }
    let all_tools = vec![dummy_tool("read"), dummy_tool("subagent")];
    let agent = |tools: Option<Vec<String>>| AgentMetadata {
        name: "a".into(),
        description: String::new(),
        location: String::new(),
        tools,
        disallowed_tools: None,
        system_prompt: String::new(),
    };
    for tools in [None, Some(vec!["read".into(), "subagent".into()])] {
        let names: Vec<String> = filter_tools_for_agent(&all_tools, &agent(tools.clone()))
            .iter()
            .map(|t| t.name().to_string())
            .collect();
        assert_eq!(names, vec!["read"], "tools: {tools:?}");
    }
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
    assert!(!ok_res.is_error);
    assert!(
        ok_res
            .content
            .starts_with("Incomplete: subagent reached the turn limit of 0 before finishing"),
        "{}",
        ok_res.content
    );
    assert!(ok_res.content.contains("Synthesized findings after turn exhaustion."));
    assert!(
        ok_res
            .content
            .ends_with("[subagent: incomplete, 0 turns, 0 tool calls, 0 tokens, test-provider/test-subagent-model]")
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

/// A running round's mailbox `task`, whose delegating agent is `boss`, plus the registry to send through.
fn round_mailbox() -> (
    crate::mailbox::Mailboxes,
    crate::mailbox::Mailbox,
    crate::mailbox::Mailbox,
) {
    let boxes = crate::mailbox::Mailboxes::default();
    let boss = boxes.open("boss", None);
    let task = boxes.open("task", Some("boss"));
    (boxes, boss, task)
}

fn send(boxes: &crate::mailbox::Mailboxes, from: crate::mailbox::Party, body: &str) {
    boxes
        .send(crate::mailbox::Envelope::new(
            from,
            "task",
            crate::mailbox::Kind::Command,
            body,
        ))
        .unwrap();
}

#[tokio::test]
async fn a_message_waiting_at_the_final_answer_keeps_the_subagent_working() {
    let adapter = MockAdapter::from_turns([text_turn("first answer"), text_turn("revised answer")]);
    let (boxes, _boss, inbox) = round_mailbox();
    send(
        &boxes,
        crate::mailbox::Party::Agent("boss".into()),
        "also cover the error path",
    );
    let runner = SubagentRunner::new(Arc::new(adapter.clone()), SubagentRole::Scout, &[], 5).with_inbox(inbox.clone());

    let result = runner.run("review", None).await;

    assert!(result.content.starts_with("revised answer"), "{}", result.content);
    let requests = adapter.requests();
    assert_eq!(requests.len(), 2);
    let sent = serde_json::to_string(&requests[1].messages).unwrap();
    assert!(sent.contains("first answer"), "{sent}");
    assert!(
        sent.contains("Message from the delegating agent:\\nalso cover the error path"),
        "{sent}"
    );
    assert!(inbox.take().is_empty());
}

#[tokio::test]
async fn a_message_arriving_with_the_answer_on_the_last_turn_leaves_the_answer_and_the_message() {
    let adapter = MockAdapter::from_turns([text_turn("final answer")]);
    let (boxes, _boss, inbox) = round_mailbox();
    send(&boxes, crate::mailbox::Party::Agent("boss".into()), "too late");
    let runner = SubagentRunner::new(Arc::new(adapter.clone()), SubagentRole::Scout, &[], 1).with_inbox(inbox.clone());

    let result = runner.run("review", None).await;

    assert!(!result.is_error, "{}", result.content);
    assert!(result.content.starts_with("final answer"), "{}", result.content);
    assert_eq!(adapter.requests().len(), 1);
    assert_eq!(inbox.take()[0].body, "too late", "left for the undelivered report");
}

#[tokio::test]
async fn a_message_from_the_user_reaches_the_round_labelled_as_from_the_user() {
    let adapter = MockAdapter::from_turns([text_turn("first answer"), text_turn("revised answer")]);
    let (boxes, _boss, inbox) = round_mailbox();
    send(&boxes, crate::mailbox::Party::User, "check the tests too");
    let runner = SubagentRunner::new(Arc::new(adapter.clone()), SubagentRole::Scout, &[], 5).with_inbox(inbox);

    runner.run("review", None).await;

    let sent = serde_json::to_string(&adapter.requests()[1].messages).unwrap();
    assert!(sent.contains("Message from the user:\\ncheck the tests too"), "{sent}");
    assert!(!sent.contains("delegating agent"), "{sent}");
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

    assert_eq!(
        tool_outputs(&adapter.requests()[1]),
        vec!["Error: Tool 'nope' not found"]
    );
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

fn agent(name: &str, tools: Option<&[&str]>) -> rho_harness_core::agents::AgentMetadata {
    rho_harness_core::agents::AgentMetadata {
        name: name.to_string(),
        description: String::new(),
        location: String::new(),
        tools: tools.map(|t| t.iter().map(ToString::to_string).collect()),
        disallowed_tools: None,
        system_prompt: format!("You are {name}."),
    }
}

fn named_tool(name: &'static str) -> Arc<dyn EngineTool> {
    counting_tool(name, Arc::new(AtomicUsize::new(0)), "ok".into())
}

fn role_tool(resolver: SubagentModelResolver, smol: Option<&str>, slow: Option<&str>) -> SubagentTool {
    SubagentTool::new(Some(Arc::new(MockAdapter::text("from session"))), Vec::new())
        .with_agents(vec![agent("reviewer", None)])
        .with_model_resolver(resolver)
        .with_model_roles(smol.map(str::to_string), slow.map(str::to_string))
}

fn acme_resolver() -> SubagentModelResolver {
    let handles: std::collections::HashMap<&str, ModelHandle> = [
        ("acme/smol", Arc::new(MockAdapter::text("from smol")) as ModelHandle),
        ("acme/slow", Arc::new(MockAdapter::text("from slow")) as ModelHandle),
    ]
    .into();
    Arc::new(move |model| {
        handles
            .get(model)
            .cloned()
            .ok_or_else(|| format!("unexpected model {model}"))
    })
}

fn persona(name: &str, instructions: &str) -> rho_harness_core::agents::PersonaMetadata {
    rho_harness_core::agents::PersonaMetadata {
        name: name.to_string(),
        description: format!("{name} behavior"),
        location: String::new(),
        instructions: instructions.to_string(),
    }
}

#[tokio::test]
async fn a_persona_adds_its_instructions_to_the_role_and_keeps_the_role_tools() {
    let adapter = MockAdapter::text("reviewed");
    let tool = SubagentTool::new(
        Some(Arc::new(adapter.clone())),
        vec![named_tool("read"), named_tool("write")],
    )
    .with_personas(vec![persona("skeptic", "Demand evidence for every claim.")]);
    assert!(
        tool.description().contains("- Personas (pass the name as `persona`"),
        "{}",
        tool.description()
    );
    assert!(tool.description().contains("skeptic (skeptic behavior)"));

    let result = tool
        .execute(json!({ "role": "critic", "persona": "Skeptic", "prompt": "review" }))
        .await
        .unwrap();

    assert!(result.content.starts_with("reviewed"), "{}", result.content);
    let request = &adapter.requests()[0];
    let preamble = request.preamble.as_deref().unwrap();
    let critic = SubagentRole::Critic.system_instructions().trim();
    assert!(
        preamble.starts_with(&format!(
            "{critic}\n\n## Persona: skeptic\nDemand evidence for every claim."
        )),
        "{preamble}"
    );
    let tools: Vec<&str> = request.tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(tools, ["read"]);
}

#[tokio::test]
async fn unknown_personas_are_rejected_with_valid_options() {
    let bare = SubagentTool::new(Some(Arc::new(MockAdapter::text("x"))), Vec::new());
    let with_one = bare.clone().with_personas(vec![persona("skeptic", "doubt")]);
    let cases = [
        (&bare, "none are defined (add .agents/personas/<name>.md)"),
        (&with_one, "valid: skeptic"),
    ];
    for (tool, expected) in cases {
        let result = tool
            .execute(json!({ "persona": "cynic", "prompt": "go" }))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("unknown persona 'cynic'"), "{}", result.content);
        assert!(result.content.contains(expected), "{}", result.content);
    }
}

#[tokio::test]
async fn a_pin_to_an_unconfigured_model_role_names_the_pin_in_the_error() {
    let pins: BTreeMap<String, ModelRole> = [("critic".to_string(), ModelRole::Slow)].into();
    let tool = role_tool(acme_resolver(), Some("acme/smol"), None).with_pinned_models(pins);

    let pinned = tool.execute(json!({ "role": "critic", "prompt": "go" })).await.unwrap();
    let asked = tool
        .execute(json!({ "role": "critic", "prompt": "go", "model": "slow" }))
        .await
        .unwrap();

    assert!(pinned.is_error);
    assert!(
        pinned
            .content
            .ends_with("model role 'slow' is not configured (set models.slow in config.toml); it is pinned for 'critic' in [subagents.models]"),
        "{}",
        pinned.content
    );
    assert!(asked.is_error && !asked.content.contains("pinned"), "{}", asked.content);
}

#[test]
fn pins_that_match_no_role_agent_or_persona_are_reported_as_unused() {
    let pins: BTreeMap<String, ModelRole> = [
        ("critic".to_string(), ModelRole::Slow),
        ("reviewer".to_string(), ModelRole::Slow),
        ("skeptic".to_string(), ModelRole::Slow),
        ("typo-scuot".to_string(), ModelRole::Slow),
    ]
    .into();
    let tool = role_tool(acme_resolver(), None, None)
        .with_personas(vec![persona("Skeptic", "doubt")])
        .with_pinned_models(pins);

    assert_eq!(tool.unused_pins(), ["typo-scuot"]);
}

#[tokio::test]
async fn a_persona_pin_outranks_the_role_pin_but_not_an_explicit_model() {
    let pins: BTreeMap<String, ModelRole> = [
        ("skeptic".to_string(), ModelRole::Slow),
        ("critic".to_string(), ModelRole::Default),
    ]
    .into();
    let cases = [
        (
            json!({ "role": "critic", "persona": "skeptic", "prompt": "go" }),
            "from slow",
        ),
        (
            json!({ "role": "critic", "persona": "skeptic", "prompt": "go", "model": "smol" }),
            "from smol",
        ),
        (
            json!({ "role": "critic", "persona": "terse", "prompt": "go" }),
            "from session",
        ),
    ];
    for (args, expected) in cases {
        let tool = role_tool(acme_resolver(), Some("acme/smol"), Some("acme/slow"))
            .with_pinned_models(pins.clone())
            .with_personas(vec![persona("skeptic", "doubt"), persona("terse", "short")]);
        let result = tool.execute(args.clone()).await.unwrap();
        assert!(result.content.starts_with(expected), "{args}: {}", result.content);
    }
}

#[tokio::test]
async fn configured_pins_pick_the_model_role_unless_the_caller_passes_one() {
    let pins: BTreeMap<String, ModelRole> = [
        ("reviewer".to_string(), ModelRole::Slow),
        ("critic".to_string(), ModelRole::Default),
    ]
    .into();
    let cases = [
        (json!({ "agent": "Reviewer", "prompt": "go" }), "from slow"),
        (
            json!({ "agent": "reviewer", "prompt": "go", "model": "smol" }),
            "from smol",
        ),
        (json!({ "role": "critic", "prompt": "go" }), "from session"),
        (json!({ "role": "scout", "prompt": "go" }), "from smol"),
    ];
    for (args, expected) in cases {
        let tool = role_tool(acme_resolver(), Some("acme/smol"), Some("acme/slow")).with_pinned_models(pins.clone());
        let result = tool.execute(args.clone()).await.unwrap();
        assert!(result.content.starts_with(expected), "{args}: {}", result.content);
    }
}

#[tokio::test]
async fn subagent_models_are_chosen_by_role() {
    let cases = [
        (
            json!({ "agent": "reviewer", "prompt": "go" }),
            Some("acme/smol"),
            "from smol",
        ),
        (json!({ "role": "scout", "prompt": "go" }), None, "from session"),
        (
            json!({ "agent": "reviewer", "prompt": "go", "model": "slow" }),
            Some("acme/smol"),
            "from slow",
        ),
        (
            json!({ "agent": "reviewer", "prompt": "go", "model": "default" }),
            Some("acme/smol"),
            "from session",
        ),
        (
            json!({ "agent": "reviewer", "prompt": "go", "model": "smol" }),
            Some(" acme/smol "),
            "from smol",
        ),
    ];
    for (args, smol, expected) in cases {
        let tool = role_tool(acme_resolver(), smol, Some("acme/slow"));
        let result = tool.execute(args.clone()).await.unwrap();
        assert!(result.content.starts_with(expected), "{args}: {}", result.content);
    }
}

#[tokio::test]
async fn unknown_or_unconfigured_roles_and_unresolvable_models_are_errors() {
    let failing: SubagentModelResolver = Arc::new(|_| Err("no credentials".into()));
    let cases = [
        (
            role_tool(acme_resolver(), None, None),
            "slow",
            "model role 'slow' is not configured",
        ),
        (
            role_tool(acme_resolver(), None, None),
            "claude/opus",
            "model must be one of default, smol, slow",
        ),
        (
            role_tool(acme_resolver(), None, None),
            "guard",
            "model must be one of default, smol, slow",
        ),
        (
            role_tool(failing, Some("acme/m1"), None),
            "smol",
            "failed to resolve model 'acme/m1' for subagent: no credentials",
        ),
    ];
    for (tool, model, expected) in cases {
        let result = tool
            .execute(json!({ "agent": "reviewer", "prompt": "go", "model": model }))
            .await
            .unwrap();
        assert!(result.is_error, "{model}");
        assert!(result.content.contains(expected), "{model}: {}", result.content);
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
        agent("reader", Some(&["read", "rg"])),
        agent("writer", Some(&["read", "write"])),
        agent("everything", None),
        agent("toolless", Some(&[])),
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

#[test]
fn subagents_with_their_own_worktrees_all_run_in_parallel() {
    let tool = SubagentTool::new(None, vec![named_tool("read"), named_tool("write")]).with_worktrees(
        crate::worktree::WorktreeStore::new(std::env::temp_dir().join("rho-unused")),
    );
    let sessions = tempfile::tempdir().unwrap();
    let session = rho_harness_core::session::SessionManager::new(sessions.path(), None).unwrap();
    let in_project = |git_status: Option<&str>, session: Option<rho_harness_core::session::SessionManager>| {
        let mut project = crate::engine::context::ProjectContext::for_tests(std::path::Path::new("."));
        project.git_status = git_status.map(str::to_string);
        tool.with_turn_context(&TurnToolContext {
            project: Some(Arc::new(project)),
            session,
            ..TurnToolContext::default()
        })
    };
    let in_git = in_project(Some("## main"), Some(session.clone()));
    let outside_git = in_project(None, Some(session));
    let unsaved = in_project(Some("## main"), None);
    let cases = [
        (json!({ "role": "general", "prompt": "p" }), true, false),
        (json!({ "task_id": "t-1", "prompt": "p" }), true, false),
        (json!({ "role": "scout", "prompt": "p" }), true, true),
        (json!({ "prompt": 7 }), false, false),
    ];
    for (args, git, plain) in cases {
        assert_eq!(in_git.concurrency_safe(&args), git, "in git: {args}");
        assert_eq!(outside_git.concurrency_safe(&args), plain, "outside git: {args}");
        assert_eq!(
            unsaved.concurrency_safe(&args),
            plain,
            "no session, so no worktree: {args}"
        );
    }
    assert!(
        !tool.concurrency_safe(&json!({ "role": "general", "prompt": "p" })),
        "no project"
    );
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

        let prompt = adapter.requests()[0].asked();
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

    let prompt = adapter.requests()[0].asked();
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
        .with_agents(vec![agent("reviewer", Some(&["read"]))])
        .bind_turn(&ctx)
        .unwrap();

    tool.execute(json!({ "agent": "reviewer", "prompt": "go" }))
        .await
        .unwrap();

    assert_eq!(
        *progress.0.lock().unwrap(),
        vec!["subagent reviewer turn 1/100: read\n"]
    );
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

#[tokio::test]
async fn named_subagents_label_progress_the_monitor_and_the_result() {
    let adapter = MockAdapter::from_turns([tool_call_turn(&[("read", "x")]), text_turn("done")]);
    let progress = Arc::new(CollectedProgress::default());
    let monitor = rho_harness_core::agents::AgentMonitor::default();
    let ctx = TurnToolContext {
        progress: ToolStreamPort::new(Some(progress.clone())),
        monitor: Some(monitor.clone()),
        ..Default::default()
    };
    let tool = SubagentTool::new(Some(Arc::new(adapter)), vec![named_tool("read")])
        .bind_turn(&ctx)
        .unwrap();

    let result = tool
        .execute(json!({ "name": "performance audit", "role": "scout", "prompt": "go" }))
        .await
        .unwrap();

    assert_eq!(
        *progress.0.lock().unwrap(),
        vec!["subagent performanceAudit turn 1/100: read\n"]
    );
    let summaries = monitor.summaries();
    assert_eq!(summaries[0].label, "performanceAudit · scout");
    assert_eq!(summaries[0].status, rho_harness_core::agents::AgentStatus::Done);
    assert!(
        result
            .content
            .ends_with("[subagent performanceAudit: 2 turns, 1 tool calls, 0 tokens, mock-provider/mock-model]"),
        "{}",
        result.content
    );
}

fn edit_tool(fails_for: &'static str) -> Arc<dyn EngineTool> {
    Arc::new(GenericTool::new("edit", "edits", json!({}), move |args| async move {
        if args["path"] == fails_for {
            return Ok(ToolResult::error("oldText not found"));
        }
        Ok(ToolResult::success("edited"))
    }))
}

fn edit_turn(paths: &[&str]) -> Vec<ModelStreamEvent> {
    paths
        .iter()
        .enumerate()
        .map(|(i, path)| {
            ModelStreamEvent::ToolCall(ToolCall::new(
                format!("call-{i}"),
                ToolFunction::new("edit", json!({ "path": path })),
            ))
        })
        .collect()
}

#[tokio::test]
async fn results_list_changed_files_and_failed_tool_calls() {
    let adapter = MockAdapter::from_turns([
        edit_turn(&["src/a.rs", "./src/../src/missing.rs"]),
        text_turn("fixed it"),
    ]);
    let runner = SubagentRunner::new(
        Arc::new(adapter),
        SubagentRole::General,
        &[edit_tool("./src/../src/missing.rs")],
        5,
    );

    let result = runner.run("fix", None).await;

    assert_eq!(
        result.content,
        "fixed it\n\nFiles changed: src/a.rs\nTool outcomes: 1 ok, 1 error (edit src/missing.rs: oldText not found)\n\n[subagent: 1 failed tool call, 2 turns, 2 tool calls, 0 tokens, mock-provider/mock-model]"
    );
}

#[tokio::test]
async fn an_empty_final_answer_is_salvaged_instead_of_discarding_the_work() {
    let adapter = MockAdapter::from_turns([
        edit_turn(&["src/a.rs"]),
        text_turn(""),
        text_turn("Summary: edited src/a.rs to fix the parser."),
    ]);
    let monitor = rho_harness_core::agents::AgentMonitor::default();
    let runner = SubagentRunner::new(Arc::new(adapter.clone()), SubagentRole::General, &[edit_tool("")], 5)
        .with_context(TurnToolContext {
            monitor: Some(monitor.clone()),
            ..Default::default()
        });

    let result = runner.run("fix the parser", None).await;

    assert!(!result.is_error, "{}", result.content);
    assert!(
        result
            .content
            .starts_with("Incomplete: subagent ended without writing a final answer before finishing")
    );
    assert!(result.content.contains("Summary: edited src/a.rs to fix the parser."));
    assert!(result.content.contains("Files changed: src/a.rs"));
    assert!(result.content.contains("incomplete, 2 turns, 1 tool calls"));
    assert_eq!(adapter.requests().len(), 3, "one extra call to summarize");
    assert_eq!(
        monitor.summaries()[0].status,
        rho_harness_core::agents::AgentStatus::InputRequired
    );
}
