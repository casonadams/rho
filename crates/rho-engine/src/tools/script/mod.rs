pub mod filter;
pub mod runner;
#[cfg(test)]
mod tests;

pub use filter::{FilterResult, filter_lines};
pub use runner::{ScriptDispatcher, ToolRunner, execute_script, format_pipe_header};

use crate::tools::types::{ToolResult, generated_schema, into_dynamic_result};
use rho_harness_core::args::ScriptArgs;
use rho_harness_core::error::AppError;
use rig::tool::DynamicTool;
use std::sync::Arc;

pub struct ScriptTool {
    dispatcher: Arc<ScriptDispatcher>,
    max_output_bytes: usize,
}

impl ScriptTool {
    pub fn new(dispatcher: Arc<ScriptDispatcher>, max_output_bytes: usize) -> Self {
        Self {
            dispatcher,
            max_output_bytes,
        }
    }

    pub async fn execute(&self, args: ScriptArgs) -> Result<ToolResult, AppError> {
        execute_script(&self.dispatcher, args, self.max_output_bytes).await
    }
}

pub fn build_script_dynamic_tool(dispatcher: Arc<ScriptDispatcher>, max_output_bytes: usize) -> DynamicTool {
    DynamicTool::new(
        "script",
        "Execute an ordered sequence of tool calls in one turn with optional regex output filtering.",
        generated_schema::<ScriptArgs>(),
        move |_ctx, args| {
            let dispatcher = Arc::clone(&dispatcher);
            Box::pin(async move {
                let parsed: ScriptArgs = match serde_json::from_value(args) {
                    Ok(a) => a,
                    Err(e) => {
                        return into_dynamic_result(Ok(ToolResult::error(format!(
                            "failed to parse script arguments: {e}"
                        ))));
                    }
                };
                into_dynamic_result(execute_script(&dispatcher, parsed, max_output_bytes).await)
            })
        },
    )
}
