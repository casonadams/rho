use rho_harness_core::config::Config;
use rho_harness_core::error::Result;

use rho_harness_core::session::SessionManager;
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
    pub built_in_tools: Option<Vec<rig::tool::DynamicTool>>,
}

pub fn build_coding_agent(model: ModelHandle, config: &Config, runtime: CodingRuntime<'_>) -> Result<Agent> {
    let CodingRuntime {
        memory, built_in_tools, ..
    } = runtime;

    let builder = AgentBuilder::from_model_handle(model)
        .memory(memory)
        .default_max_turns(config.max_turns)
        .record_content_telemetry(false)
        .dynamic_tools(built_in_tools.unwrap_or_default());

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
