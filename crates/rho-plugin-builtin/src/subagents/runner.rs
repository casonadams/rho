use super::types::{AgentExecutionResult, AgentTemplate};
use async_trait::async_trait;
use rho_core::config::Config;
use rho_core::error::Result;
use rho_sdk::contract::ToolHost;
use std::sync::Arc;

pub fn resolve_subagent_model<'a>(
    config: &'a Config,
    template: &'a AgentTemplate,
    model_override: Option<&'a str>,
) -> &'a str {
    model_override
        .or(template.model.as_deref())
        .or(config.subagents.default_model.as_deref())
        .unwrap_or(&config.model)
}

pub struct SubagentExecuteRequest<'a> {
    pub job_id: Option<&'a str>,
    pub template: &'a AgentTemplate,
    pub prompt: &'a str,
    pub model_override: Option<&'a str>,
    pub steering_rx: Option<Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<String>>>>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_template(model: Option<&str>) -> AgentTemplate {
        AgentTemplate {
            name: "test-agent".to_string(),
            description: "A test agent".to_string(),
            system_prompt: "You are a test agent".to_string(),
            tools: vec!["read".to_string()],
            model: model.map(str::to_string),
        }
    }

    #[test]
    fn test_resolve_model_explicit_override_wins() {
        let mut config = Config::default();
        config.model = "parent-model".to_string();
        config.subagents.default_model = Some("subagents-default".to_string());
        let template = dummy_template(Some("template-model"));

        let resolved = resolve_subagent_model(&config, &template, Some("explicit-override"));
        assert_eq!(resolved, "explicit-override");
    }

    #[test]
    fn test_resolve_model_template_wins_over_default_and_parent() {
        let mut config = Config::default();
        config.model = "parent-model".to_string();
        config.subagents.default_model = Some("subagents-default".to_string());
        let template = dummy_template(Some("template-model"));

        let resolved = resolve_subagent_model(&config, &template, None);
        assert_eq!(resolved, "template-model");
    }

    #[test]
    fn test_resolve_model_subagents_default_wins_over_parent() {
        let mut config = Config::default();
        config.model = "parent-model".to_string();
        config.subagents.default_model = Some("subagents-default".to_string());
        let template = dummy_template(None);

        let resolved = resolve_subagent_model(&config, &template, None);
        assert_eq!(resolved, "subagents-default");
    }

    #[test]
    fn test_resolve_model_fallback_to_parent_model() {
        let mut config = Config::default();
        config.model = "parent-model".to_string();
        config.subagents.default_model = None;
        let template = dummy_template(None);

        let resolved = resolve_subagent_model(&config, &template, None);
        assert_eq!(resolved, "parent-model");
    }
}
