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

    let extras = crate::provider::provider_request_extras(
        &config.provider,
        config.thinking_level.as_deref(),
        &memory.session_id,
    );
    let builder = AgentBuilder::from_model_handle(model)
        .memory(memory)
        .default_max_turns(config.max_turns)
        .record_content_telemetry(false)
        .dynamic_tools(built_in_tools.unwrap_or_default());
    let builder = match extras {
        Some(extras) => builder.additional_params(extras),
        None => builder,
    };

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

    #[tokio::test]
    async fn coding_agent_carries_chatgpt_reasoning_and_cache_key() {
        let model = MockCompletionModel::text("done");
        let (config, dir) = coding_config("chatgpt", Some("high"));
        let agent = coding_agent_with_model(model.clone(), &config, &dir);
        build_runner(&agent, "prompt").run().await.unwrap();

        let requests = model.requests();
        let params = requests[0].additional_params.as_ref().unwrap();
        assert_eq!(params["reasoning"]["effort"], "high");
        assert_eq!(params["reasoning"]["summary"], "auto");
        assert!(params["prompt_cache_key"].as_str().is_some_and(|k| !k.is_empty()));
    }

    #[tokio::test]
    async fn coding_agent_keeps_cache_key_when_thinking_off() {
        let model = MockCompletionModel::text("done");
        let (config, dir) = coding_config("chatgpt", Some("off"));
        let agent = coding_agent_with_model(model.clone(), &config, &dir);
        build_runner(&agent, "prompt").run().await.unwrap();

        let requests = model.requests();
        let params = requests[0].additional_params.as_ref().unwrap();
        assert!(params.get("reasoning").is_none());
        assert!(params.get("prompt_cache_key").is_some());
    }

    #[tokio::test]
    async fn coding_agent_carries_anthropic_thinking_budget() {
        let model = MockCompletionModel::text("done");
        let (config, dir) = coding_config("anthropic", Some("medium"));
        let agent = coding_agent_with_model(model.clone(), &config, &dir);
        build_runner(&agent, "prompt").run().await.unwrap();

        let requests = model.requests();
        let params = requests[0].additional_params.as_ref().unwrap();
        assert_eq!(
            params["thinking"],
            serde_json::json!({ "type": "enabled", "budget_tokens": 4096 })
        );
    }

    #[tokio::test]
    async fn coding_agent_sends_no_extras_for_own_clients() {
        for provider in ["claude", "antigravity"] {
            let model = MockCompletionModel::text("done");
            let (config, dir) = coding_config(provider, Some("high"));
            let agent = coding_agent_with_model(model.clone(), &config, &dir);
            build_runner(&agent, "prompt").run().await.unwrap();

            let requests = model.requests();
            assert!(requests[0].additional_params.is_none(), "{provider}");
        }
    }

    fn coding_config(provider: &str, thinking: Option<&str>) -> (Config, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("rho_extras_{}_{}", provider, uuid::Uuid::new_v4()));
        let config = Config {
            provider: provider.to_string(),
            thinking_level: thinking.map(str::to_string),
            sessions_dir: dir.join("sessions"),
            ..Config::default()
        };
        (config, dir)
    }

    fn coding_agent_with_model(model: MockCompletionModel, config: &Config, dir: &std::path::Path) -> Agent {
        build_coding_agent(
            ModelHandle::new(model),
            config,
            CodingRuntime {
                base_dir: dir,
                memory: SessionManager::new(&dir.join("sessions"), None).unwrap(),
                built_in_tools: None,
            },
        )
        .unwrap()
    }
}
