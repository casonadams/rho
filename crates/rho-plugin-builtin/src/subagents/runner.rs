use super::types::{AgentExecutionResult, AgentTemplate};
use async_trait::async_trait;
use rho_core::error::Result;
use rho_sdk::contract::ToolHost;
use std::sync::Arc;

pub struct SubagentExecuteRequest<'a> {
    pub job_id: Option<&'a str>,
    pub template: &'a AgentTemplate,
    pub prompt: &'a str,
    pub model_override: Option<&'a str>,
}

#[async_trait]
pub trait SubagentExecutor: Send + Sync {
    async fn execute(&self, request: SubagentExecuteRequest<'_>, host: &dyn ToolHost) -> Result<AgentExecutionResult>;
}

#[derive(Clone)]
pub struct SubagentRunner {
    executor: Arc<dyn SubagentExecutor>,
    pub max_turns: usize,
}

impl SubagentRunner {
    pub fn new(executor: Arc<dyn SubagentExecutor>, max_turns: usize) -> Self {
        Self { executor, max_turns }
    }

    pub async fn run(&self, request: SubagentExecuteRequest<'_>, host: &dyn ToolHost) -> Result<AgentExecutionResult> {
        let job_id = request.job_id.unwrap_or_default().to_string();
        let mut result = self.executor.execute(request, host).await?;
        result.job_id = job_id;
        Ok(result)
    }
}

#[derive(Default)]
pub struct NoopExecutor;

#[async_trait]
impl SubagentExecutor for NoopExecutor {
    async fn execute(&self, request: SubagentExecuteRequest<'_>, host: &dyn ToolHost) -> Result<AgentExecutionResult> {
        let text = format!("Subagent completed task: {}", request.prompt);
        host.stream_chunk(&text);
        Ok(AgentExecutionResult {
            job_id: request.job_id.unwrap_or_default().to_string(),
            status: "completed".to_string(),
            text,
            tool_calls_count: 0,
            is_error: false,
        })
    }
}

// Re-export for compatibility
pub use NoopExecutor as NoopProvider;
