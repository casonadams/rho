use crate::engine::compactor::llm::ModelHandle;
use crate::tools::subagent::types::SubagentRole;
use crate::tools::types::ToolResult;
use rig::tool::DynamicTool;
use std::time::Duration;

pub fn build_subagent_prompt(prompt: &str, context_slice: Option<&str>) -> String {
    match context_slice {
        Some(slice) if !slice.trim().is_empty() => {
            format!("Context:\n{}\n\nTask:\n{}", slice.trim(), prompt.trim())
        }
        _ => prompt.trim().to_string(),
    }
}

pub fn filter_tools_for_role(tools: &[DynamicTool], role: SubagentRole) -> Vec<DynamicTool> {
    let allowed = role.allowed_tools();
    let mut filtered: Vec<DynamicTool> = tools
        .iter()
        .filter(|t| allowed.contains(&t.name().as_str()))
        .cloned()
        .collect();
    filtered.sort_by(|a, b| a.name().cmp(b.name()));
    filtered
}

pub struct SubagentRunner {
    model: ModelHandle,
    role: SubagentRole,
    tools: Vec<DynamicTool>,
    max_turns: usize,
}

impl SubagentRunner {
    #[must_use]
    pub fn new(model: ModelHandle, role: SubagentRole, available_tools: &[DynamicTool], max_turns: usize) -> Self {
        let tools = filter_tools_for_role(available_tools, role);
        Self {
            model,
            role,
            tools,
            max_turns,
        }
    }

    #[must_use]
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name().as_str()).collect()
    }

    pub async fn run(&self, prompt: &str, context_slice: Option<&str>) -> ToolResult {
        if prompt.trim().is_empty() {
            return ToolResult::error("subagent prompt cannot be empty");
        }

        let full_prompt = build_subagent_prompt(prompt, context_slice);
        let tool_server = rig::tool::server::ToolServer::new().dynamic_tools(self.tools.clone());
        let tool_handle = tool_server.run();

        let agent = rig::agent::AgentBuilder::new(self.model.clone())
            .preamble(self.role.system_instructions())
            .default_max_turns(self.max_turns)
            .record_content_telemetry(false)
            .tool_server_handle(tool_handle)
            .build();

        let runner = crate::engine::runtime::build_runner(&agent, full_prompt).max_turns(self.max_turns);
        let timeout = Duration::from_secs(120);

        match tokio::time::timeout(timeout, runner.run()).await {
            Ok(Ok(response)) => {
                let output = response.output();
                let text = output.trim();
                if text.is_empty() {
                    ToolResult::error("Subagent completed without generating an output summary.")
                } else {
                    ToolResult::success(text)
                }
            }
            Ok(Err(err)) => ToolResult::error(format!("Subagent execution failed: {err}")),
            Err(_) => ToolResult::error("Subagent execution timed out after 120s."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::rig::model::into_dyn_model;
    use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelCompletionResponse, ModelStreamEvent};
    use rho_harness_core::error::AppError;

    struct EchoMockAdapter {
        reply: String,
    }

    #[async_trait::async_trait]
    impl ModelAdapter for EchoMockAdapter {
        fn model_name(&self) -> &str {
            "mock-subagent"
        }

        fn provider_name(&self) -> &str {
            "mock"
        }

        async fn complete(&self, _request: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError> {
            Ok(ModelCompletionResponse {
                content: self.reply.clone(),
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
                Ok(ModelStreamEvent::Text(self.reply.clone())),
                Ok(ModelStreamEvent::Usage(
                    crate::engine::metrics::StructuralUsage::default(),
                )),
            ];
            Ok(Box::pin(futures::stream::iter(events)))
        }
    }

    #[test]
    fn test_prompt_formatting() {
        assert_eq!(build_subagent_prompt("do work", None), "do work");
        assert_eq!(build_subagent_prompt("do work", Some("")), "do work");
        assert_eq!(
            build_subagent_prompt("do work", Some("important background")),
            "Context:\nimportant background\n\nTask:\ndo work"
        );
    }

    #[tokio::test]
    async fn test_subagent_runner_empty_prompt() {
        let model = into_dyn_model(EchoMockAdapter { reply: "done".into() });
        let runner = SubagentRunner::new(model, SubagentRole::Scout, &[], 5);
        let res = runner.run("   ", None).await;
        assert!(res.is_error);
        assert!(res.content.contains("subagent prompt cannot be empty"));
    }

    #[tokio::test]
    async fn test_subagent_runner_success() {
        let model = into_dyn_model(EchoMockAdapter {
            reply: "Found 3 files matching pattern.".into(),
        });
        let runner = SubagentRunner::new(model, SubagentRole::Scout, &[], 5);
        let res = runner.run("find files", Some("search in src/")).await;
        assert!(!res.is_error);
        assert_eq!(res.content, "Found 3 files matching pattern.");
    }
}
