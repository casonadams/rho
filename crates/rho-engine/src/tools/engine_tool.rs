use crate::engine::runner::turn::types::CancellationSignal;
use crate::hook::LifecycleHook;
use crate::permission::PermissionHook;
use crate::tools::types::{ToolResult, generated_schema, parse_tool_args};
use rho_harness_core::agents::AgentMonitor;
use rho_harness_core::error::AppError;
use rho_harness_core::model::ChatMessage;
use rho_harness_core::presentation::ToolStreamPort;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use std::sync::Arc;

/// Turn-scoped state a tool needs when it runs other tools on the model's behalf.
#[derive(Clone, Default)]
pub struct TurnToolContext {
    pub permission: Option<Arc<PermissionHook>>,
    pub lifecycle: Option<Arc<LifecycleHook>>,
    pub cancellation: Option<CancellationSignal>,
    pub progress: ToolStreamPort,
    pub parent_history: Arc<[ChatMessage]>,
    pub monitor: Option<AgentMonitor>,
    pub project: Option<Arc<crate::engine::context::ProjectContext>>,
    /// The parent session, where subagent tasks are saved so they can be continued.
    pub session: Option<rho_harness_core::session::SessionManager>,
    /// The running agent's mailbox; background work a tool starts reports to it.
    pub inbox: Option<crate::mailbox::Mailbox>,
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

/// A built-in tool whose arguments are one typed struct. The blanket `EngineTool` impl supplies the
/// name, catalog description, schema and argument parsing, so a tool only writes `run`.
#[async_trait::async_trait]
pub trait TypedTool: Send + Sync {
    type Args: DeserializeOwned + JsonSchema + Send;
    const NAME: &'static str;
    /// Whether calls may run alongside other concurrency-safe calls in the same batch.
    const CONCURRENCY_SAFE: bool = false;

    async fn run(&self, args: Self::Args) -> Result<ToolResult, AppError>;

    /// Returns a copy of this tool bound to the current turn, or `None` if the tool is turn-independent.
    fn bind_to_turn(&self, _ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        None
    }
}

#[async_trait::async_trait]
impl<T: TypedTool> EngineTool for T {
    fn name(&self) -> &str {
        T::NAME
    }

    fn description(&self) -> &str {
        crate::tools::ToolRegistry::builtin_description(T::NAME)
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<T::Args>()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, AppError> {
        match parse_tool_args::<T::Args>(T::NAME, args, || self.parameters()) {
            Ok(args) => self.run(args).await,
            Err(invalid) => Ok(invalid),
        }
    }

    fn bind_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        self.bind_to_turn(ctx)
    }

    fn concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        T::CONCURRENCY_SAFE
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{bash::BashTool, read::ReadTool};

    #[tokio::test]
    async fn typed_tools_supply_name_description_schema_and_parsing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
        let read: &dyn EngineTool = &ReadTool::new(dir.path());

        assert_eq!(read.name(), "read");
        assert_eq!(
            read.description(),
            crate::tools::ToolRegistry::builtin_description("read")
        );
        assert_eq!(read.parameters()["properties"]["path"]["type"], "string");
        assert!(
            read.execute(serde_json::json!({ "path": "a.txt" }))
                .await
                .unwrap()
                .content
                .contains("hello")
        );

        let invalid = read.execute(serde_json::json!({})).await.unwrap();
        assert!(invalid.is_error);
        assert!(
            invalid.content.contains("Invalid arguments for read"),
            "{}",
            invalid.content
        );
    }

    #[test]
    fn typed_tools_declare_whether_their_calls_may_run_together() {
        let dir = tempfile::tempdir().unwrap();
        let none = serde_json::json!({});
        assert!(ReadTool::new(dir.path()).concurrency_safe(&none));
        assert!(!BashTool::new(dir.path()).concurrency_safe(&none));
    }
}
