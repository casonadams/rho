use super::super::supervisor::SubagentSupervisor;
use rho_sdk::capability::{CapabilityError, CapabilityId, CapabilityKind};
use rho_sdk::contract::{
    ExecutionMode, OperationEffect, ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest,
    ToolInvocationResponse,
};
use serde::{Deserialize, Serialize};

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
