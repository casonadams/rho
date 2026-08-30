#![cfg(unix)]

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use rho::config::{Config, SubagentsConfig};
use rho::plugin::tool_dispatch::ActiveToolSet;
use rho_sdk::capability::CapabilityError;
use rho_sdk::contract::{
    AuthenticationRequest, AuthenticationResponse, FinishReason, ProviderCapability, ProviderDescriptor,
    ProviderRequest, ProviderStreamEvent,
};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct FixtureProvider {
    turns: Mutex<VecDeque<Vec<Result<ProviderStreamEvent, CapabilityError>>>>,
    requests: Mutex<Vec<ProviderRequest>>,
}

impl FixtureProvider {
    fn new(turns: impl IntoIterator<Item = Vec<ProviderStreamEvent>>) -> Self {
        Self {
            turns: Mutex::new(
                turns
                    .into_iter()
                    .map(|turn| turn.into_iter().map(Ok).collect())
                    .collect(),
            ),
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ProviderCapability for FixtureProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "provider:fixture".parse().unwrap(),
            display_name: "Fixture".to_string(),
            models: Vec::new(),
            authentication: Vec::new(),
        }
    }

    async fn authenticate(&self, _request: AuthenticationRequest) -> Result<AuthenticationResponse, CapabilityError> {
        unreachable!()
    }

    async fn stream(
        &self,
        request: ProviderRequest,
    ) -> Result<BoxStream<'static, Result<ProviderStreamEvent, CapabilityError>>, CapabilityError> {
        self.requests.lock().unwrap().push(request);
        let turn = self.turns.lock().unwrap().pop_front().unwrap_or_default();
        Ok(Box::pin(stream::iter(turn)))
    }
}

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("subagents_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn test_subagents_tools_registered_when_enabled() {
    let workspace = temp_workspace();
    let config = Config {
        subagents: SubagentsConfig {
            enabled: true,
            max_concurrency: 4,
            max_turns_per_agent: 20,
            ..SubagentsConfig::default()
        },
        auto_approve: true,
        ..Config::default()
    };

    let active = ActiveToolSet::load(&config, &workspace).await.unwrap();
    let names: Vec<String> = active.definitions().iter().map(|d| d.id.name().to_string()).collect();

    assert!(names.contains(&"agent".to_string()));
    assert!(names.contains(&"get_subagent_result".to_string()));
    assert!(names.contains(&"steer_subagent".to_string()));

    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn test_subagents_tools_omitted_when_disabled() {
    let workspace = temp_workspace();
    let config = Config {
        subagents: SubagentsConfig {
            enabled: false,
            ..SubagentsConfig::default()
        },
        ..Config::default()
    };

    let active = ActiveToolSet::load(&config, &workspace).await.unwrap();
    let names: Vec<String> = active.definitions().iter().map(|d| d.id.name().to_string()).collect();

    assert!(!names.contains(&"agent".to_string()));
    assert!(!names.contains(&"get_subagent_result".to_string()));
    assert!(!names.contains(&"steer_subagent".to_string()));

    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn test_foreground_subagent_execution_end_to_end() {
    let workspace = temp_workspace();
    let provider = Arc::new(FixtureProvider::new([vec![
        ProviderStreamEvent::TextDelta {
            text: "Found auth middleware in src/auth.rs".to_string(),
        },
        ProviderStreamEvent::Finished {
            reason: FinishReason::Stop,
        },
    ]]));

    let config = Config {
        subagents: SubagentsConfig {
            enabled: true,
            max_concurrency: 4,
            max_turns_per_agent: 10,
            ..SubagentsConfig::default()
        },
        auto_approve: true,
        ..Config::default()
    };

    let runner = Arc::new(rho_plugin_builtin::subagents::SubagentRunner::new(
        provider,
        config.subagents.max_turns_per_agent,
    ));
    let supervisor = rho_plugin_builtin::subagents::SubagentSupervisor::new(runner, config.subagents.max_concurrency);

    let tools = rho_plugin_builtin::subagents::create_subagent_tools(supervisor, &config, &workspace);
    let agent_tool = &tools[0].1;

    struct DummyHost;
    #[async_trait]
    impl rho_sdk::contract::ToolHost for DummyHost {
        async fn interact(
            &self,
            _request: rho_sdk::contract::InteractionRequest,
        ) -> Result<rho_sdk::contract::InteractionResponse, CapabilityError> {
            unreachable!()
        }
    }

    let response = agent_tool
        .invoke(
            &DummyHost,
            rho_sdk::contract::ToolInvocationRequest {
                arguments: serde_json::json!({
                    "subagent_type": "explore",
                    "prompt": "find auth",
                    "run_in_background": false
                }),
                context: rho_sdk::contract::InvocationContext {
                    session_id: "test".to_string(),
                    working_directory: ".".to_string(),
                    has_interactive_ui: false,
                },
            },
        )
        .await
        .unwrap();

    assert_eq!(response.content, "Found auth middleware in src/auth.rs");
    assert!(!response.is_error);

    let _ = std::fs::remove_dir_all(workspace);
}
