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

type GenericToolHandler = Arc<
    dyn Fn(
            serde_json::Value,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult, AppError>> + Send>>
        + Send
        + Sync,
>;

pub struct GenericTool {
    name: String,
    description: String,
    parameters: serde_json::Value,
    handler: GenericToolHandler,
}

impl GenericTool {
    pub fn new<F, Fut>(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
        handler: F,
    ) -> Self
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<ToolResult, AppError>> + Send + 'static,
    {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
            handler: Arc::new(move |args| Box::pin(handler(args))),
        }
    }
}

#[async_trait::async_trait]
impl EngineTool for GenericTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        self.parameters.clone()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, AppError> {
        (self.handler)(args).await
    }
}
