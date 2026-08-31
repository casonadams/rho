use super::super::discovery::discover_templates;
use super::super::supervisor::{BackgroundTaskRequest, SubagentSupervisor, SubagentTaskRequest};
use super::super::types::AgentInvocationArgs;
use rho_core::config::Config;
use rho_sdk::capability::{CapabilityError, CapabilityId, CapabilityKind};
use rho_sdk::contract::{
    ExecutionMode, OperationEffect, ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest,
    ToolInvocationResponse,
};
use std::path::Path;

pub static PROMPT_AGENT: &str = include_str!("../../../../../prompts/tools/agent.md");

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
            .unwrap_or_else(|| super::super::types::AgentTemplate {
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
