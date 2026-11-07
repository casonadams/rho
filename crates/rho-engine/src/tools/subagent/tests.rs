use super::*;
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelCompletionResponse, ModelStreamEvent};
use rho_harness_core::model::{AssistantContent, ChatMessage, ToolCall, ToolFunction};
use serde_json::json;

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
        assert_eq!(result.content, answer);
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
        model: Some("claude-3-5-sonnet".to_string()),
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
    assert_eq!(res.content, "Code reviewer: Clean code, no issues found");

    // 2. Invoking via "role" matching custom agent name
    let args_role = json!({
        "role": "code-reviewer",
        "prompt": "Review PR 42"
    });
    let res = subagent.execute(args_role).await.unwrap();
    assert!(!res.is_error);
    assert_eq!(res.content, "Code reviewer: Clean code, no issues found");
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
    assert_eq!(ok_res.content, "Synthesized findings after turn exhaustion.");

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
    let sanitized = runner::prepare_history_for_recovery(&history);
    assert_eq!(sanitized.len(), 1);
    assert_eq!(sanitized[0], valid_msg);
}

#[tokio::test]
async fn test_dispatch_subagent_tool_calls() {
    let calls = vec![ToolCall::new("call_1", ToolFunction::new("unknown_tool", json!({})))];
    let res = runner::dispatch_subagent_tool_calls(&calls, &[]).await;
    assert_eq!(res.len(), 1);
    match &res[0] {
        ChatMessage::User { content } => match &content[0] {
            rho_harness_core::model::UserContent::ToolResult(tr) => {
                match &tr.content[0] {
                    rho_harness_core::model::ToolResultContent::Text(t) => {
                        assert!(t.text.contains("Unknown tool 'unknown_tool'"));
                    }
                    _ => panic!("Expected text content"),
                }
                assert_eq!(tr.call, "call_1");
            }
            _ => panic!("Expected ToolResult"),
        },
        _ => panic!("Expected User message"),
    }
}
