use crate::engine::runner::turn::types::CancellationSignal;
use crate::hook::LifecycleHook;
use crate::permission::PermissionHook;
use crate::tools::types::ToolResult;
use rho_harness_core::error::AppError;
use rho_harness_core::model::ChatMessage;
use rho_harness_core::presentation::ToolStreamPort;
use std::sync::Arc;

/// Turn-scoped state a tool needs when it runs other tools on the model's behalf.
#[derive(Clone, Default)]
pub struct TurnToolContext {
    pub permission: Option<Arc<PermissionHook>>,
    pub lifecycle: Option<Arc<LifecycleHook>>,
    pub cancellation: Option<CancellationSignal>,
    pub progress: ToolStreamPort,
    pub parent_history: Arc<[ChatMessage]>,
}

#[async_trait::async_trait]
pub trait EngineTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, AppError>;

    /// Returns a copy of this tool bound to the current turn, or `None` if the tool is turn-independent.
    fn bind_turn(&self, _ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        None
    }

    /// Whether a call with these arguments may run alongside other concurrency-safe calls in the same batch.
    fn concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        false
    }
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

    fn bind_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        (**self).bind_turn(ctx)
    }

    fn concurrency_safe(&self, args: &serde_json::Value) -> bool {
        (**self).concurrency_safe(args)
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
