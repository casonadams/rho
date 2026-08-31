pub mod store;
#[cfg(test)]
mod tests;
pub mod types;

pub use store::TodoStore;
pub use types::{TaskStatus, TodoAction, TodoArgs, TodoCreateParams, TodoTask, TodoUpdateParams};

use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use async_trait::async_trait;
use rho_sdk::capability::{CapabilityError, CapabilityId, CapabilityKind};
use rho_sdk::contract::{
    ExecutionMode, ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest, ToolInvocationResponse,
};
use rig::tool::{Tool, ToolContext, ToolExecutionError};

pub static PROMPT_TODO: &str = include_str!("../../../../../prompts/tools/todo.md");

pub struct TodoTool {
    store: TodoStore,
    descriptor: ToolDescriptor,
}

impl TodoTool {
    pub fn new(store: TodoStore) -> Self {
        let schema = generated_schema::<TodoArgs>();
        let descriptor = ToolDescriptor {
            id: CapabilityId::new(CapabilityKind::Tool, "todo").unwrap(),
            description: "Manage a task list for tracking multi-step progress. Actions: create, update, list, get, delete, clear.".to_string(),
            argument_schema: schema,
            prompt_guidance: PROMPT_TODO.to_string(),
            effects: Vec::new(),
            execution_mode: ExecutionMode::Sequential,
        };

        Self { store, descriptor }
    }

    pub async fn execute(&self, args: TodoArgs) -> Result<ToolResult, rho_core::error::AppError> {
        match args.action {
            TodoAction::Create => {
                let Some(subject) = args.subject.filter(|s| !s.trim().is_empty()) else {
                    return Ok(ToolResult::error("Task 'subject' is required for create"));
                };
                match self.store.create(TodoCreateParams {
                    subject,
                    description: args.description,
                    status: args.status,
                    active_form: args.active_form,
                    owner: args.owner,
                    blocked_by: args.blocked_by,
                    metadata: args.metadata,
                }) {
                    Ok(msg) => Ok(ToolResult::success(msg)),
                    Err(e) => Ok(ToolResult::error(e)),
                }
            }
            TodoAction::Update => {
                let Some(id) = args.id else {
                    return Ok(ToolResult::error("Task 'id' is required for update"));
                };
                if args.subject.is_none()
                    && args.description.is_none()
                    && args.status.is_none()
                    && args.active_form.is_none()
                    && args.owner.is_none()
                    && args.add_blocked_by.is_none()
                    && args.remove_blocked_by.is_none()
                    && args.metadata.is_none()
                {
                    return Ok(ToolResult::error("At least one field to update must be provided"));
                }
                match self.store.update(TodoUpdateParams {
                    id,
                    subject: args.subject,
                    description: args.description,
                    status: args.status,
                    active_form: args.active_form,
                    owner: args.owner,
                    add_blocked_by: args.add_blocked_by,
                    remove_blocked_by: args.remove_blocked_by,
                    metadata: args.metadata,
                }) {
                    Ok(msg) => Ok(ToolResult::success(msg)),
                    Err(e) => Ok(ToolResult::error(e)),
                }
            }
            TodoAction::List => {
                let output = self.store.list(args.status, args.include_deleted.unwrap_or(false));
                Ok(ToolResult::success(output))
            }
            TodoAction::Get => {
                let Some(id) = args.id else {
                    return Ok(ToolResult::error("Task 'id' is required for get"));
                };
                match self.store.get(id) {
                    Ok(msg) => Ok(ToolResult::success(msg)),
                    Err(e) => Ok(ToolResult::error(e)),
                }
            }
            TodoAction::Delete => {
                let Some(id) = args.id else {
                    return Ok(ToolResult::error("Task 'id' is required for delete"));
                };
                match self.store.delete(id) {
                    Ok(msg) => Ok(ToolResult::success(msg)),
                    Err(e) => Ok(ToolResult::error(e)),
                }
            }
            TodoAction::Clear => Ok(ToolResult::success(self.store.clear())),
        }
    }
}

impl Tool for TodoTool {
    const NAME: &'static str = "todo";
    type Args = TodoArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Manage a task list for tracking multi-step progress. Actions: create, update, list, get, delete, clear."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<TodoArgs>()
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}

#[async_trait]
impl ToolCapability for TodoTool {
    fn descriptor(&self) -> ToolDescriptor {
        self.descriptor.clone()
    }

    async fn invoke(
        &self,
        _host: &dyn ToolHost,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse, CapabilityError> {
        let args: TodoArgs =
            serde_json::from_value(request.arguments).map_err(|e| CapabilityError::InvalidRequest {
                message: format!("Invalid todo arguments: {e}"),
            })?;

        match self.execute(args).await {
            Ok(res) => Ok(ToolInvocationResponse {
                content: res.content,
                is_error: res.is_error,
                structured_content: res.metadata,
            }),
            Err(e) => Err(CapabilityError::Failed { message: e.to_string() }),
        }
    }
}
