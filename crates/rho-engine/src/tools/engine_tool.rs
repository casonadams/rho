use crate::tools::types::ToolResult;
use rho_harness_core::error::AppError;
use std::sync::Arc;

#[async_trait::async_trait]
pub trait EngineTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, AppError>;
}

#[async_trait::async_trait]
impl<T: ?Sized + EngineTool> EngineTool for Arc<T> {
    fn name(&self) -> &str {
        (**self).name()
    }

    fn description(&self) -> &str {
        (**self).description()
    }

    fn parameters(&self) -> serde_json::Value {
        (**self).parameters()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, AppError> {
        (**self).execute(args).await
    }
}
