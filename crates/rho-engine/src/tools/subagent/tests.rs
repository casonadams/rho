use super::*;
use crate::adapter::rig::model::into_dyn_model;
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelCompletionResponse, ModelStreamEvent};
use serde_json::json;

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
    let model = into_dyn_model(TestModel {
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
    fn dummy_tool(name: &'static str) -> DynamicTool {
        DynamicTool::new(name, "desc", json!({}), |_| {
            Box::pin(async { Ok(rig::tool::ToolOutput::text("ok")) })
        })
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

    let model = into_dyn_model(TestModel { answer: "ok".into() });

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
    let model = into_dyn_model(TestModel {
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
    let dyn_res = crate::adapter::rig::tools::into_dynamic_result(Ok(res)).unwrap();
    assert_eq!(dyn_res.as_text(), Some("Scout report: zero defects found"));
}
