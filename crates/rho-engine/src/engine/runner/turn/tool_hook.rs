use crate::engine::runner::sink::{TerminalApprovalSink, ToolFinishDetails};
use crate::provider::supports_tool_result_images;
use rig::agent::hook::{AgentHook, ToolResultAction};
use rig::completion::message::{Image, MimeType, ToolResultContent};
use rig::tool::ToolOutput;

/// Renders the sink display and decides the model-visible action for every
/// tool result. Image blocks are kept only for providers whose rig adapter
/// serializes them; for everyone else they are replaced with an omission note
/// so the request cannot fail with "does not support images in tool results".
pub struct TurnToolExecutionHook {
    sink: std::sync::Arc<TerminalApprovalSink>,
    provider: String,
}

impl TurnToolExecutionHook {
    pub fn new(sink: std::sync::Arc<TerminalApprovalSink>, provider: &str) -> Self {
        Self {
            sink,
            provider: provider.to_string(),
        }
    }
}

impl AgentHook for TurnToolExecutionHook {
    async fn on_tool_call(
        &self,
        _ctx: &rig::agent::hook::HookContext,
        event: rig::agent::hook::ToolCall<'_>,
    ) -> rig::agent::hook::ToolCallAction {
        let arguments = serde_json::from_str(event.args).unwrap_or(serde_json::Value::Null);
        self.sink.tool_start(event.tool_name, &arguments);
        rig::agent::hook::ToolCallAction::run()
    }

    async fn on_tool_result(
        &self,
        _ctx: &rig::agent::hook::HookContext,
        event: rig::agent::hook::ToolResultEvent<'_>,
    ) -> ToolResultAction {
        let arguments = serde_json::from_str(event.args).unwrap_or(serde_json::Value::Null);
        let (action, output) = gated_result(event.presentation, &self.provider);
        let is_error = !event.raw_result.is_success();
        self.sink.tool_finished(ToolFinishDetails {
            name: event.tool_name,
            arguments: &arguments,
            output: &output,
            is_error,
        });
        action
    }
}

/// The model-visible action and transcript text for a tool result.
///
/// Results without image blocks pass through untouched. Image-bearing results
/// render text-only (base64 never leaks into the display); for providers that
/// cannot serialize them, the rewrite strips every image block and appends an
/// omission note.
fn gated_result(presentation: &ToolOutput, provider: &str) -> (ToolResultAction, String) {
    let (text, has_images) = text_render(presentation);
    if !has_images || supports_tool_result_images(provider) {
        return (ToolResultAction::keep(), text);
    }
    let gated = with_omission_note(&text, provider);
    (ToolResultAction::rewrite(gated.clone()), gated)
}

/// Text-only rendering of a tool output: identical to `ToolOutput::render`
/// unless image blocks are present, in which case text parts are joined with
/// newlines and each image contributes a compact placeholder.
fn text_render(presentation: &ToolOutput) -> (String, bool) {
    let blocks = presentation.as_content();
    let has_images = blocks.iter().any(|block| matches!(block, ToolResultContent::Image(_)));
    if !has_images {
        return (presentation.render(), false);
    }
    let parts: Vec<String> = blocks
        .iter()
        .map(|block| match block {
            ToolResultContent::Text(text) => text.text.clone(),
            ToolResultContent::Image(image) => image_placeholder(image),
            ToolResultContent::Json { value } => value.to_string(),
        })
        .collect();
    (parts.join("\n"), true)
}

fn image_placeholder(image: &Image) -> String {
    let media_type = image.media_type.as_ref().map_or("unknown", MimeType::to_mime_type);
    format!("[image: {media_type}]")
}

fn with_omission_note(text: &str, provider: &str) -> String {
    let note = format!("[Image in tool result omitted: {provider} does not support images in tool results.]");
    if text.is_empty() {
        return note;
    }
    format!("{text}\n{note}")
}

#[cfg(test)]
mod tests;
