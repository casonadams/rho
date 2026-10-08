#[cfg(test)]
mod tests;

use crate::adapter::rig::parse_media_type;
use crate::tools::EngineTool;
use rho_harness_core::error::AppError;
use rig::tool::{DynamicTool, ToolExecutionError};
use std::sync::Arc;

pub fn into_dynamic_result(
    result: Result<crate::tools::ToolResult, AppError>,
) -> Result<rig::tool::ToolOutput, ToolExecutionError> {
    match result {
        Ok(result) if result.is_error => {
            if result.content.starts_with("failed to parse tool arguments:") {
                Err(ToolExecutionError::invalid_args(result.content))
            } else if result.content.contains("permission denied") || result.content.contains("access denied") {
                Err(ToolExecutionError::permission_denied(result.content))
            } else if result.content.contains("not found") {
                Err(ToolExecutionError::not_found(result.content))
            } else if result.content.contains("timed out") || result.content.contains("timeout") {
                Err(ToolExecutionError::timeout(result.content))
            } else {
                Err(ToolExecutionError::other(result.content))
            }
        }
        Ok(result) => Ok(tool_output(result)),
        Err(AppError::Policy(msg)) => Err(ToolExecutionError::permission_denied(msg)),
        Err(AppError::Cancelled(msg)) => Err(ToolExecutionError::cancelled(msg)),
        Err(error) => Err(ToolExecutionError::from_error(error)),
    }
}

pub fn tool_output(result: crate::tools::ToolResult) -> rig::tool::ToolOutput {
    let Some(image) = result.image else {
        return rig::tool::ToolOutput::text(result.content);
    };
    let media_type = parse_media_type(&image.mime);
    rig::tool::ToolOutput::content(vec![
        rig::completion::message::ToolResultContent::text(result.content),
        rig::completion::message::ToolResultContent::image_base64(image.data, media_type, None),
    ])
    .expect("text plus image block is never empty")
}

pub fn into_dynamic_tool<T: EngineTool + 'static>(tool: T) -> DynamicTool {
    into_dynamic_tool_arc(Arc::new(tool))
}

pub fn into_dynamic_tool_arc(tool: Arc<dyn EngineTool>) -> DynamicTool {
    let name = tool.name().to_string();
    let description = tool.description().to_string();
    let parameters = tool.parameters();
    let tool_name =
        rig::message::ToolName::new(&name).unwrap_or_else(|_| rig::message::ToolName::new("unknown").unwrap());

    DynamicTool::new_with_context(tool_name, description, parameters, move |_ctx, args| {
        let t = tool.clone();
        Box::pin(async move {
            let res = t.execute(args).await;
            into_dynamic_result(res)
        })
    })
}

pub fn into_streaming_dynamic_tool<T: EngineTool + 'static, F>(tool: Arc<T>, streamer: F) -> DynamicTool
where
    F: Fn(
            &mut rig::tool::ToolContext,
            &T,
            serde_json::Value,
        ) -> futures::future::BoxFuture<'static, Result<crate::tools::ToolResult, AppError>>
        + Send
        + Sync
        + 'static,
{
    let name = tool.name().to_string();
    let description = tool.description().to_string();
    let parameters = tool.parameters();
    let tool_name =
        rig::message::ToolName::new(&name).unwrap_or_else(|_| rig::message::ToolName::new("unknown").unwrap());

    DynamicTool::new_with_context(tool_name, description, parameters, move |ctx, args| {
        let t = tool.clone();
        let fut = streamer(ctx, &t, args);
        Box::pin(async move {
            let res = fut.await;
            into_dynamic_result(res)
        })
    })
}
