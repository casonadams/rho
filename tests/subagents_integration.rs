#![cfg(unix)]

use async_trait::async_trait;
use rho::config::{Config, SubagentsConfig};
use rho::plugin::tool_dispatch::ActiveToolSet;
use rho_plugin_builtin::subagents::{
    AgentExecutionResult, SubagentExecuteRequest, SubagentExecutor, SubagentRunner, SubagentSupervisor,
    create_subagent_tools, resolve_subagent_model,
};
use rho_sdk::capability::CapabilityError;
use rho_sdk::contract::ToolHost;
use std::path::PathBuf;
use std::sync::Arc;

struct FixtureExecutor {
    response_text: String,
}

#[async_trait]
impl SubagentExecutor for FixtureExecutor {
    async fn execute(
        &self,
        _request: SubagentExecuteRequest<'_>,
        host: &dyn ToolHost,
    ) -> rho_core::error::Result<AgentExecutionResult> {
        host.stream_chunk(&self.response_text);
        Ok(AgentExecutionResult {
            job_id: String::new(),
            status: "completed".to_string(),
            text: self.response_text.clone(),
            tool_calls_count: 0,
            is_error: false,
        })
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
    assert!(names.contains(&"todo".to_string()));

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
    assert!(names.contains(&"todo".to_string()));

    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn test_foreground_subagent_execution_end_to_end() {
    let workspace = temp_workspace();
    let executor = Arc::new(FixtureExecutor {
        response_text: "Found auth middleware in src/auth.rs".to_string(),
    });

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

    let runner = Arc::new(SubagentRunner::new(executor, config.subagents.max_turns_per_agent));
    let supervisor = SubagentSupervisor::new(runner, config.subagents.max_concurrency);

    let tools = create_subagent_tools(supervisor, &config, &workspace);
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

        fn stream_chunk(&self, _chunk: &str) {}
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
                    plugin_config: None,
                },
            },
        )
        .await
        .unwrap();

    assert_eq!(response.content, "Found auth middleware in src/auth.rs");
    assert!(!response.is_error);

    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn test_background_subagent_execution_and_steering() {
    let workspace = temp_workspace();
    let executor = Arc::new(FixtureExecutor {
        response_text: "Background task completed".to_string(),
    });

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

    let runner = Arc::new(SubagentRunner::new(executor, config.subagents.max_turns_per_agent));
    let supervisor = SubagentSupervisor::new(runner, config.subagents.max_concurrency);

    let tools = create_subagent_tools(supervisor.clone(), &config, &workspace);
    let agent_tool = &tools[0].1;
    let get_result_tool = &tools[1].1;
    let steer_tool = &tools[2].1;

    struct DummyHost;
    #[async_trait]
    impl rho_sdk::contract::ToolHost for DummyHost {
        async fn interact(
            &self,
            _request: rho_sdk::contract::InteractionRequest,
        ) -> Result<rho_sdk::contract::InteractionResponse, CapabilityError> {
            unreachable!()
        }

        fn stream_chunk(&self, _chunk: &str) {}
    }

    let response = agent_tool
        .invoke(
            &DummyHost,
            rho_sdk::contract::ToolInvocationRequest {
                arguments: serde_json::json!({
                    "subagent_type": "explore",
                    "prompt": "search files",
                    "run_in_background": true
                }),
                context: rho_sdk::contract::InvocationContext {
                    session_id: "test".to_string(),
                    working_directory: ".".to_string(),
                    has_interactive_ui: false,
                    plugin_config: None,
                },
            },
        )
        .await
        .unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&response.content).unwrap();
    let job_id = parsed["job_id"].as_str().unwrap();
    assert_eq!(parsed["status"], "running");

    // Steer the running subagent
    let steer_response = steer_tool
        .invoke(
            &DummyHost,
            rho_sdk::contract::ToolInvocationRequest {
                arguments: serde_json::json!({
                    "agent_id": job_id,
                    "message": "focus on src/lib.rs"
                }),
                context: rho_sdk::contract::InvocationContext {
                    session_id: "test".to_string(),
                    working_directory: ".".to_string(),
                    has_interactive_ui: false,
                    plugin_config: None,
                },
            },
        )
        .await
        .unwrap();
    assert!(!steer_response.is_error);

    // Wait for subagent to complete
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let result_response = get_result_tool
        .invoke(
            &DummyHost,
            rho_sdk::contract::ToolInvocationRequest {
                arguments: serde_json::json!({
                    "agent_id": job_id
                }),
                context: rho_sdk::contract::InvocationContext {
                    session_id: "test".to_string(),
                    working_directory: ".".to_string(),
                    has_interactive_ui: false,
                    plugin_config: None,
                },
            },
        )
        .await
        .unwrap();

    let result_parsed: serde_json::Value = serde_json::from_str(&result_response.content).unwrap();
    assert_eq!(result_parsed["status"], "completed");
    assert_eq!(result_parsed["text"], "Background task completed");

    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn test_subagent_model_resolution_and_override_propagation() {
    let workspace = temp_workspace();
    let last_override = Arc::new(std::sync::Mutex::new(None));
    let last_template_model = Arc::new(std::sync::Mutex::new(None));

    struct ModelCapturingExecutor {
        last_override: Arc<std::sync::Mutex<Option<String>>>,
        last_template_model: Arc<std::sync::Mutex<Option<String>>>,
    }

    #[async_trait]
    impl SubagentExecutor for ModelCapturingExecutor {
        async fn execute(
            &self,
            request: SubagentExecuteRequest<'_>,
            host: &dyn ToolHost,
        ) -> rho_core::error::Result<AgentExecutionResult> {
            *self.last_override.lock().unwrap() = request.model_override.map(str::to_string);
            *self.last_template_model.lock().unwrap() = request.template.model.clone();
            host.stream_chunk("model captured");
            Ok(AgentExecutionResult {
                job_id: String::new(),
                status: "completed".to_string(),
                text: "model captured".to_string(),
                tool_calls_count: 0,
                is_error: false,
            })
        }
    }

    let executor = Arc::new(ModelCapturingExecutor {
        last_override: Arc::clone(&last_override),
        last_template_model: Arc::clone(&last_template_model),
    });

    let mut config = Config {
        model: "parent-claude-model".to_string(),
        subagents: SubagentsConfig {
            enabled: true,
            max_concurrency: 4,
            max_turns_per_agent: 10,
            default_model: Some("subagent-fast-default".to_string()),
            ..SubagentsConfig::default()
        },
        auto_approve: true,
        ..Config::default()
    };

    let runner = Arc::new(SubagentRunner::new(executor, config.subagents.max_turns_per_agent));
    let supervisor = SubagentSupervisor::new(runner, config.subagents.max_concurrency);

    let tools = create_subagent_tools(supervisor, &config, &workspace);
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

        fn stream_chunk(&self, _chunk: &str) {}
    }

    // 1. Invocation with explicit model override
    agent_tool
        .invoke(
            &DummyHost,
            rho_sdk::contract::ToolInvocationRequest {
                arguments: serde_json::json!({
                    "subagent_type": "explore",
                    "prompt": "find auth",
                    "model": "explicit-override-model",
                    "run_in_background": false
                }),
                context: rho_sdk::contract::InvocationContext {
                    session_id: "test".to_string(),
                    working_directory: ".".to_string(),
                    has_interactive_ui: false,
                    plugin_config: None,
                },
            },
        )
        .await
        .unwrap();

    assert_eq!(
        last_override.lock().unwrap().as_deref(),
        Some("explicit-override-model")
    );

    // 2. Invocation with template without model (explore defaults to None) -> inherits subagents.default_model
    agent_tool
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
                    plugin_config: None,
                },
            },
        )
        .await
        .unwrap();

    assert_eq!(last_override.lock().unwrap().as_deref(), None);
    assert_eq!(last_template_model.lock().unwrap().as_deref(), None);

    let explore_template = rho_plugin_builtin::subagents::AgentTemplate {
        name: "explore".to_string(),
        description: "explore".to_string(),
        system_prompt: "prompt".to_string(),
        tools: vec![],
        model: None,
    };
    let resolved_explore = resolve_subagent_model(&config, &explore_template, None);
    assert_eq!(resolved_explore, "subagent-fast-default");

    // 3. Invocation with template that explicitly specifies a model -> template model wins over default_model
    let template_with_model = rho_plugin_builtin::subagents::AgentTemplate {
        name: "specialized-worker".to_string(),
        description: "specialized".to_string(),
        system_prompt: "prompt".to_string(),
        tools: vec![],
        model: Some("custom-specialized-model".to_string()),
    };
    let resolved_custom = resolve_subagent_model(&config, &template_with_model, None);
    assert_eq!(resolved_custom, "custom-specialized-model");

    // 3. Invocation with ad-hoc agent type (no template model) -> uses subagents.default_model
    agent_tool
        .invoke(
            &DummyHost,
            rho_sdk::contract::ToolInvocationRequest {
                arguments: serde_json::json!({
                    "subagent_type": "adhoc-worker",
                    "prompt": "do adhoc work",
                    "run_in_background": false
                }),
                context: rho_sdk::contract::InvocationContext {
                    session_id: "test".to_string(),
                    working_directory: ".".to_string(),
                    has_interactive_ui: false,
                    plugin_config: None,
                },
            },
        )
        .await
        .unwrap();

    assert_eq!(last_override.lock().unwrap().as_deref(), None);
    assert_eq!(last_template_model.lock().unwrap().as_deref(), None);

    let adhoc_template = rho_plugin_builtin::subagents::AgentTemplate {
        name: "adhoc-worker".to_string(),
        description: "adhoc".to_string(),
        system_prompt: "prompt".to_string(),
        tools: vec![],
        model: None,
    };
    let resolved = resolve_subagent_model(&config, &adhoc_template, None);
    assert_eq!(resolved, "subagent-fast-default");

    // 4. Fallback to parent model when default_model is None
    config.subagents.default_model = None;
    let resolved_fallback = resolve_subagent_model(&config, &adhoc_template, None);
    assert_eq!(resolved_fallback, "parent-claude-model");

    let _ = std::fs::remove_dir_all(workspace);
}
