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
async fn test_subagent_tool_depth_limit() {
    let tool = SubagentTool::new(None, 1, Vec::new());
    let args = json!({
        "prompt": "search codebase"
    });
    let result = tool.execute(args).await.unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("Maximum subagent recursion depth"));
}

#[tokio::test]
async fn test_subagent_tool_missing_model() {
    let tool = SubagentTool::new(None, 0, Vec::new());
    let args = json!({
        "prompt": "search codebase"
    });
    let result = tool.execute(args).await.unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("No model handle configured"));
}

#[tokio::test]
async fn test_subagent_tool_invalid_args() {
    let tool = SubagentTool::new(None, 0, Vec::new());
    let args = json!({
        "prompt": 12345
    });
    let result = tool.execute(args).await.unwrap();
    assert!(result.is_error);
    assert!(result.content.contains("failed to parse subagent arguments"));
}

#[tokio::test]
async fn test_subagent_tool_success() {
    let model = Arc::new(TestModel {
        answer: "Scout completed: identified 4 services".into(),
    });
    let tool = SubagentTool::new(Some(model), 0, Vec::new());
    let args = json!({
        "role": "scout",
        "prompt": "Find all service definitions",
        "context_slice": "Check crates/rho-engine"
    });
    let result = tool.execute(args).await.unwrap();
    assert!(!result.is_error);
    assert_eq!(result.content, "Scout completed: identified 4 services");
}

#[test]
fn test_subagent_tool_metadata() {
    let tool = SubagentTool::new(None, 0, Vec::new());
    assert_eq!(tool.name(), "subagent");
    assert!(!tool.description().is_empty());
    assert_eq!(tool.depth(), 0);
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

    let model = Arc::new(TestModel { answer: "ok".into() });

    let scout = SubagentRunner::new(model.clone(), SubagentRole::Scout, &all_tools, 5);
    assert_eq!(scout.tool_names(), vec!["fd", "read", "rg", "web_fetch", "web_search"]);
    assert!(!scout.tool_names().contains(&"write"));
    assert!(!scout.tool_names().contains(&"bash"));

    let critic = SubagentRunner::new(model.clone(), SubagentRole::Critic, &all_tools, 5);
    assert_eq!(critic.tool_names(), vec!["fd", "read", "rg"]);
    assert!(!critic.tool_names().contains(&"write"));

    let planner = SubagentRunner::new(model.clone(), SubagentRole::Planner, &all_tools, 5);
    assert_eq!(planner.tool_names(), vec!["fd", "read", "rg"]);
    assert!(!planner.tool_names().contains(&"write"));

    let general = SubagentRunner::new(model, SubagentRole::General, &all_tools, 5);
    assert_eq!(
        general.tool_names(),
        vec!["bash", "edit", "fd", "read", "rg", "web_fetch", "web_search", "write"]
    );
}

#[tokio::test]
async fn test_subagent_as_dynamic_tool() {
    let model = Arc::new(TestModel {
        answer: "Scout report: zero defects found".into(),
    });
    let subagent = Arc::new(SubagentTool::new(Some(model), 0, Vec::new()));
    let args = json!({
        "role": "critic",
        "prompt": "verify invariant AC-001",
        "context_slice": "spec details"
    });
    let res = subagent.execute(args).await.unwrap();
    assert!(!res.is_error);
    assert_eq!(res.content, "Scout report: zero defects found");
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

    let subagent = Arc::new(SubagentTool::new(Some(model.clone()), 0, Vec::new()).with_agents(vec![custom_agent]));

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
