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

pub fn build_coding_agent(
    model: ModelHandle,
    config: &Config,
    runtime: CodingRuntime<'_>,
) -> Result<(Agent, rig::tool::server::ToolServerHandle)> {
    let CodingRuntime {
        memory, built_in_tools, ..
    } = runtime;

    let extras = crate::provider::provider_request_extras(
        &config.provider,
        config.thinking_level.as_deref(),
        &memory.session_id,
    );
    let mut tools = built_in_tools.unwrap_or_default();
    tools.sort_by(|a, b| a.name().cmp(b.name()));
    let tool_server = rig::tool::server::ToolServer::new().dynamic_tools(tools);
    let tool_handle = tool_server.run();
    let builder = AgentBuilder::from_model_handle(model)
        .memory(memory)
        .default_max_turns(config.max_turns)
        .record_content_telemetry(false)
        .tool_server_handle(tool_handle.clone());
    let builder = match extras {
        Some(extras) => builder.additional_params(extras),
        None => builder,
    };

    let builder = if config.semantic_search {
        let index_path = crate::rag::CodebaseIndex::index_path(runtime.base_dir);
        if let Some(index) = crate::rag::CodebaseIndex::load(&index_path) {
            let embedder = if index.model == "deterministic" {
                std::sync::Arc::new(crate::rag::LocalEmbedder::new_deterministic())
            } else {
                std::sync::Arc::new(crate::rag::LocalEmbedder::new())
            };
            let vector_index = crate::rag::CodebaseVectorIndex::new(std::sync::Arc::new(index), embedder);
            builder.dynamic_context(3, vector_index)
        } else {
            builder
        }
    } else {
        builder
    };

    let agent = match config.max_output_tokens {
        Some(max_tokens) => builder.max_tokens(max_tokens).build(),
        None => builder.build(),
    };
    Ok((agent, tool_handle))
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

    #[tokio::test]
    async fn coding_agent_dynamic_context_with_semantic_search() {
        let model = MockCompletionModel::text("done");
        let (mut config, dir) = coding_config("local", None);
        config.semantic_search = true;

        // Create index file
        let index_path = crate::rag::CodebaseIndex::index_path(&dir);
        let index = crate::rag::CodebaseIndex {
            version: 1,
            model: "deterministic".to_string(),
            chunks: vec![crate::rag::CodeChunk {
                id: "src/lib.rs:1-10".to_string(),
                file_path: "src/lib.rs".to_string(),
                start_line: 1,
                end_line: 10,
                content: "pub fn hello_world() {}".to_string(),
                content_hash: "abc".to_string(),
                embedding: crate::rag::deterministic_embed("pub fn hello_world() {}", 384),
            }],
        };
        index.save(&index_path).unwrap();

        let agent = coding_agent_with_model(model.clone(), &config, &dir);
        build_runner(&agent, "hello world").run().await.unwrap();

        let requests = model.requests();
        assert_eq!(requests.len(), 1);
        let doc = requests[0].documents.first().expect("document attached");
        assert!(doc.text.contains("hello_world"));
        assert!(doc.text.contains("hello_world"));
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
        .map(|(agent, _)| agent)
        .unwrap()
    }
}
