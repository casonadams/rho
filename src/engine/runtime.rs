use crate::config::Config;
use crate::error::Result;
use crate::session::SessionManager;
use crate::session::context::context_memory;
use crate::tools::web::{
    FetchCache, HttpClient, SearchRateLimiter, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool,
};
use crate::tools::{AskUserQuestionTool, AskUserTool, BashTool, EditTool, ReadTool, WriteTool};
use rig::agent::{Agent, AgentBuilder, AgentRunner, ModelHandle};
use std::path::Path;

pub fn build_agent(model: ModelHandle, config: &Config, preamble: &str) -> Agent {
    let builder = AgentBuilder::from_model_handle(model)
        .preamble(preamble)
        .default_max_turns(config.max_turns)
        .record_content_telemetry(false);

    match config.max_output_tokens {
        Some(max_tokens) => builder.max_tokens(max_tokens).build(),
        None => builder.build(),
    }
}

pub struct CodingRuntime<'a> {
    pub base_dir: &'a Path,
    pub memory: SessionManager,
}

pub fn build_coding_agent(model: ModelHandle, config: &Config, runtime: CodingRuntime<'_>) -> Result<Agent> {
    let CodingRuntime { base_dir, memory } = runtime;
    let http = HttpClient::new(config.allow_private_network)?;
    let search = WebSearchTool::new(
        http.clone(),
        SearchRateLimiter::new(config.search_min_interval_ms),
        WebSearchConfig {
            region: config.region.clone(),
            timeout_sec: config.search_timeout_sec,
        },
    );
    let fetch = WebFetchTool::new(
        http,
        FetchCache::new(60, 64),
        WebFetchConfig {
            timeout_sec: config.fetch_timeout_sec,
            max_bytes: config.fetch_max_bytes,
            default_limit: config.fetch_limit,
        },
    );
    let context_memory = context_memory(memory, config.context_window_messages, config.compaction_max_bytes);
    let builder = AgentBuilder::from_model_handle(model)
        .memory(context_memory)
        .default_max_turns(config.max_turns)
        .record_content_telemetry(false)
        .tool(ReadTool::new(base_dir))
        .tool(WriteTool::with_exclusions(
            base_dir,
            [&config.config_dir, &config.sessions_dir],
        ))
        .tool(EditTool::with_exclusions(
            base_dir,
            [&config.config_dir, &config.sessions_dir],
        ))
        .tool(BashTool::new(base_dir))
        .tool(AskUserTool::new())
        .tool(AskUserQuestionTool(AskUserTool::new()))
        .tool(search)
        .tool(fetch);

    Ok(match config.max_output_tokens {
        Some(max_tokens) => builder.max_tokens(max_tokens).build(),
        None => builder.build(),
    })
}

pub fn build_runner(agent: &Agent, prompt: impl Into<rig::message::Message>) -> AgentRunner {
    agent.runner(prompt).tool_concurrency(1).record_content_telemetry(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::completion::PromptError;
    use rig::test_utils::MockCompletionModel;

    #[tokio::test]
    async fn rig_runtime_contract_omits_default_output_cap() {
        let model = MockCompletionModel::text("done");
        let agent = build_agent(ModelHandle::new(model.clone()), &Config::default(), "system");
        build_runner(&agent, "prompt").run().await.unwrap();

        let requests = model.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].max_tokens, None);
    }

    #[tokio::test]
    async fn rig_runtime_contract_passes_explicit_output_cap() {
        let model = MockCompletionModel::text("done");
        let config = Config {
            max_output_tokens: Some(8192),
            ..Config::default()
        };
        let agent = build_agent(ModelHandle::new(model.clone()), &config, "system");
        build_runner(&agent, "prompt").run().await.unwrap();

        assert_eq!(model.requests()[0].max_tokens, Some(8192));
    }

    #[tokio::test]
    async fn rig_runtime_contract_reports_budget_exhaustion() {
        let model = MockCompletionModel::text("must not run");
        let agent = build_agent(ModelHandle::new(model.clone()), &Config::default(), "system");
        let error = build_runner(&agent, "prompt").max_turns(0).run().await.unwrap_err();

        assert!(matches!(error, PromptError::MaxTurnsError { max_turns: 0, .. }));
        assert_eq!(model.request_count(), 0);
    }
}
