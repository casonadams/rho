use super::discovery::discover_templates;
use super::supervisor::{BackgroundTaskRequest, SubagentSupervisor, SubagentTaskRequest};
use super::types::AgentInvocationArgs;
use rho_core::config::Config;
use rho_sdk::capability::{CapabilityError, CapabilityId, CapabilityKind};
use rho_sdk::contract::{
    ExecutionMode, OperationEffect, ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest,
    ToolInvocationResponse,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

pub static PROMPT_AGENT: &str = include_str!("../../../../prompts/tools/agent.md");

pub struct AgentTool {
    supervisor: SubagentSupervisor,
    config: Config,
    workspace_dir: std::path::PathBuf,
    descriptor: ToolDescriptor,
}

impl AgentTool {
    pub fn new(supervisor: SubagentSupervisor, config: Config, workspace_dir: &Path) -> Self {
        let descriptor = ToolDescriptor {
            id: CapabilityId::new(CapabilityKind::Tool, "agent").unwrap(),
            description: "Launch a specialized autonomous subagent to perform complex tasks in an isolated context."
                .to_string(),
            argument_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "subagent_type": { "type": "string", "description": "Template type: explore, plan, general-purpose, etc." },
                    "prompt": { "type": "string", "description": "The task for the subagent to perform." },
                    "description": { "type": "string", "description": "Short 3-5 word description of the task." },
                    "run_in_background": { "type": "boolean", "description": "Defaults to true. Set false to block." },
                    "model": { "type": "string", "description": "Optional model override." }
                },
                "required": ["subagent_type", "prompt"]
            }),
            prompt_guidance: PROMPT_AGENT.to_string(),
            effects: vec![OperationEffect::ExecuteProcess],
            execution_mode: ExecutionMode::Sequential,
        };

        Self {
            supervisor,
            config,
            workspace_dir: workspace_dir.to_path_buf(),
            descriptor,
        }
    }
}

#[async_trait::async_trait]
impl ToolCapability for AgentTool {
    fn descriptor(&self) -> ToolDescriptor {
        self.descriptor.clone()
    }

    async fn invoke(
        &self,
        host: &dyn ToolHost,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse, CapabilityError> {
        let args: AgentInvocationArgs =
            serde_json::from_value(request.arguments).map_err(|e| CapabilityError::InvalidRequest {
                message: format!("Invalid Agent arguments: {e}"),
            })?;

        let templates = discover_templates(&self.config, &self.workspace_dir);
        let template = templates
            .get(&args.subagent_type.to_lowercase())
            .cloned()
            .unwrap_or_else(|| super::types::AgentTemplate {
                name: args.subagent_type.clone(),
                description: "Ad-hoc subagent".to_string(),
                system_prompt: "You are an autonomous coding subagent.".to_string(),
                tools: vec!["read".to_string(), "bash".to_string()],
                model: args.model.clone(),
            });

        if args.run_in_background {
            let job_id = self
                .supervisor
                .spawn_background(BackgroundTaskRequest {
                    template,
                    prompt: args.prompt,
                    description: args.description,
                    model_override: args.model,
                })
                .map_err(|e| CapabilityError::Failed {
                    message: format!("Failed to spawn background subagent: {e}"),
                })?;

            let response_json = serde_json::json!({
                "job_id": job_id,
                "status": "running",
                "message": "Subagent spawned in background. Use get_subagent_result to check status."
            });

            Ok(ToolInvocationResponse {
                content: response_json.to_string(),
                is_error: false,
                structured_content: None,
            })
        } else {
            let res = self
                .supervisor
                .run_foreground(
                    SubagentTaskRequest {
                        template: &template,
                        prompt: &args.prompt,
                        model_override: args.model.as_deref(),
                    },
                    host,
                )
                .await
                .map_err(|e| CapabilityError::Failed {
                    message: format!("Subagent execution failed: {e}"),
                })?;

            Ok(ToolInvocationResponse {
                content: res.text,
                is_error: res.is_error,
                structured_content: None,
            })
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GetSubagentResultArgs {
    pub agent_id: String,
}

pub struct GetSubagentResultTool {
    supervisor: SubagentSupervisor,
    descriptor: ToolDescriptor,
}

impl GetSubagentResultTool {
    pub fn new(supervisor: SubagentSupervisor) -> Self {
        let descriptor = ToolDescriptor {
            id: CapabilityId::new(CapabilityKind::Tool, "get_subagent_result").unwrap(),
            description: "Check status and retrieve a background agent's result.".to_string(),
            argument_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "The job ID returned by Agent." }
                },
                "required": ["agent_id"]
            }),
            prompt_guidance: "Use get_subagent_result to check on background agents.".to_string(),
            effects: vec![OperationEffect::ExecuteProcess],
            execution_mode: ExecutionMode::Sequential,
        };

        Self { supervisor, descriptor }
    }
}

#[async_trait::async_trait]
impl ToolCapability for GetSubagentResultTool {
    fn descriptor(&self) -> ToolDescriptor {
        self.descriptor.clone()
    }

    async fn invoke(
        &self,
        _host: &dyn ToolHost,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse, CapabilityError> {
        let args: GetSubagentResultArgs =
            serde_json::from_value(request.arguments).map_err(|e| CapabilityError::InvalidRequest {
                message: format!("Invalid get_subagent_result arguments: {e}"),
            })?;

        if let Some(snapshot) = self.supervisor.get_job(&args.agent_id) {
            if let Some(result) = snapshot.result {
                let out_json = serde_json::json!({
                    "job_id": result.job_id,
                    "status": result.status,
                    "text": result.text,
                    "is_error": result.is_error
                });
                Ok(ToolInvocationResponse {
                    content: out_json.to_string(),
                    is_error: result.is_error,
                    structured_content: None,
                })
            } else {
                let out_json = serde_json::json!({
                    "job_id": args.agent_id,
                    "status": snapshot.status,
                    "message": "Agent is still running."
                });
                Ok(ToolInvocationResponse {
                    content: out_json.to_string(),
                    is_error: false,
                    structured_content: None,
                })
            }
        } else {
            Err(CapabilityError::Failed {
                message: format!("Job ID '{}' not found", args.agent_id),
            })
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SteerSubagentArgs {
    pub agent_id: String,
    pub message: String,
}

pub struct SteerSubagentTool {
    supervisor: SubagentSupervisor,
    descriptor: ToolDescriptor,
}

impl SteerSubagentTool {
    pub fn new(supervisor: SubagentSupervisor) -> Self {
        let descriptor = ToolDescriptor {
            id: CapabilityId::new(CapabilityKind::Tool, "steer_subagent").unwrap(),
            description: "Send a steering message to redirect a running background agent.".to_string(),
            argument_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "The running agent ID." },
                    "message": { "type": "string", "description": "Steering instructions to inject." }
                },
                "required": ["agent_id", "message"]
            }),
            prompt_guidance: "Use steer_subagent to send mid-run feedback to running agents.".to_string(),
            effects: vec![OperationEffect::ExecuteProcess],
            execution_mode: ExecutionMode::Sequential,
        };

        Self { supervisor, descriptor }
    }
}

#[async_trait::async_trait]
impl ToolCapability for SteerSubagentTool {
    fn descriptor(&self) -> ToolDescriptor {
        self.descriptor.clone()
    }

    async fn invoke(
        &self,
        _host: &dyn ToolHost,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse, CapabilityError> {
        let args: SteerSubagentArgs =
            serde_json::from_value(request.arguments).map_err(|e| CapabilityError::InvalidRequest {
                message: format!("Invalid steer_subagent arguments: {e}"),
            })?;

        self.supervisor
            .steer_job(&args.agent_id, &args.message)
            .map_err(|e| CapabilityError::Failed {
                message: format!("Failed to steer subagent: {e}"),
            })?;

        Ok(ToolInvocationResponse {
            content: format!("Steering message sent to agent {}", args.agent_id),
            is_error: false,
            structured_content: None,
        })
    }
}

pub fn create_subagent_tools(
    supervisor: SubagentSupervisor,
    config: &Config,
    workspace_dir: &Path,
) -> Vec<(CapabilityId, Arc<dyn ToolCapability>)> {
    if !config.subagents.enabled {
        return Vec::new();
    }

    let agent_tool = Arc::new(AgentTool::new(supervisor.clone(), config.clone(), workspace_dir));
    let get_result_tool = Arc::new(GetSubagentResultTool::new(supervisor.clone()));
    let steer_tool = Arc::new(SteerSubagentTool::new(supervisor));

    vec![
        (agent_tool.descriptor().id, agent_tool),
        (get_result_tool.descriptor().id, get_result_tool),
        (steer_tool.descriptor().id, steer_tool),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subagents::runner::{NoopExecutor, SubagentRunner};

    struct DummyHost;
    #[async_trait::async_trait]
    impl ToolHost for DummyHost {
        async fn interact(
            &self,
            _request: rho_sdk::contract::InteractionRequest,
        ) -> std::result::Result<rho_sdk::contract::InteractionResponse, CapabilityError> {
            unreachable!()
        }

        fn stream_chunk(&self, _chunk: &str) {}
    }

    #[tokio::test]
    async fn test_subagent_tool_spawns_background_and_polls_result() {
        let runner = Arc::new(SubagentRunner::new(Arc::new(NoopExecutor), 10));
        let supervisor = SubagentSupervisor::new(runner, 4);
        let config = Config::default();
        let tools = create_subagent_tools(supervisor.clone(), &config, Path::new("."));
        assert_eq!(tools.len(), 3);

        let agent_tool = &tools[0].1;
        let get_result_tool = &tools[1].1;
        let host = DummyHost;

        let res = agent_tool
            .invoke(
                &host,
                ToolInvocationRequest {
                    arguments: serde_json::json!({
                        "subagent_type": "explore",
                        "prompt": "search auth files",
                        "run_in_background": true
                    }),
                    context: rho_sdk::contract::InvocationContext::new("test", ".", false),
                },
            )
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&res.content).unwrap();
        let job_id = parsed["job_id"].as_str().unwrap();

        // Wait for background job to settle
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let poll_res = get_result_tool
            .invoke(
                &host,
                ToolInvocationRequest {
                    arguments: serde_json::json!({ "agent_id": job_id }),
                    context: rho_sdk::contract::InvocationContext::new("test", ".", false),
                },
            )
            .await
            .unwrap();

        assert!(!poll_res.is_error);
    }
}
