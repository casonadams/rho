pub mod steering;

use crate::engine::runner::sink::{TerminalApprovalSink, ToolFinishDetails};
use crate::engine::runner::turn::types::{SharedModelSwitch, SteeringQueueProvider};
use crate::provider::supports_tool_result_images;
use rig::agent::hook::{
    AgentHook, CompletionCall, CompletionCallAction, HookContext, ModelSelection, ModelSelectionAction, ToolCall,
    ToolCallAction, ToolResultAction, ToolResultEvent,
};
use rig::completion::message::{Image, MimeType, ToolResultContent};
use rig::tool::ToolOutput;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub use steering::{STEERING_SKIP_REASON, attach_steering_to_output, format_steering_messages};

/// Renders the sink display and decides the model-visible action for every
/// tool result. Image blocks are kept only for providers whose rig adapter
/// serializes them; for everyone else they are replaced with an omission note
/// so the request cannot fail with "does not support images in tool results".
pub struct TurnToolExecutionHook {
    sink: Arc<TerminalApprovalSink>,
    provider: String,
    steering: Option<Arc<dyn SteeringQueueProvider>>,
    steered: AtomicBool,
    model_switch: Option<Arc<SharedModelSwitch>>,
}

impl TurnToolExecutionHook {
    pub fn new(
        sink: Arc<TerminalApprovalSink>,
        provider: &str,
        steering: Option<Arc<dyn SteeringQueueProvider>>,
    ) -> Self {
        Self {
            sink,
            provider: provider.to_string(),
            steering,
            steered: AtomicBool::new(false),
            model_switch: None,
        }
    }

    pub fn with_model_switch(mut self, model_switch: Option<Arc<SharedModelSwitch>>) -> Self {
        self.model_switch = model_switch;
        self
    }

    pub fn is_steered(&self) -> bool {
        self.steered.load(Ordering::Relaxed)
    }

    pub fn set_steered(&self, val: bool) {
        self.steered.store(val, Ordering::Relaxed);
    }
}

impl AgentHook for TurnToolExecutionHook {
    fn on_model_select(&self, _ctx: &HookContext, _event: ModelSelection<'_>) -> ModelSelectionAction {
        if let Some(switcher) = &self.model_switch
            && let Some(handle) = switcher.get_handle()
        {
            return ModelSelectionAction::select(handle);
        }
        ModelSelectionAction::Continue
    }

    async fn on_completion_call(&self, _ctx: &HookContext, _event: CompletionCall<'_>) -> CompletionCallAction {
        self.set_steered(false);
        CompletionCallAction::continue_run()
    }

    async fn on_tool_call(&self, _ctx: &HookContext, event: ToolCall<'_>) -> ToolCallAction {
        if self.is_steered() {
            return ToolCallAction::skip(STEERING_SKIP_REASON);
        }
        if let Some(steering) = &self.steering {
            let messages = steering.poll_steering().await;
            if !messages.is_empty() {
                self.set_steered(true);
                let text = format_steering_messages(&messages);
                return ToolCallAction::skip(format!("{STEERING_SKIP_REASON}\n\n{text}"));
            }
        }
        let arguments = serde_json::from_str(event.args).unwrap_or(serde_json::Value::Null);
        self.sink.tool_start(event.tool_name, &arguments);
        ToolCallAction::run()
    }

    async fn on_tool_result(&self, _ctx: &HookContext, event: ToolResultEvent<'_>) -> ToolResultAction {
        let arguments = serde_json::from_str(event.args).unwrap_or(serde_json::Value::Null);
        let provider = self
            .model_switch
            .as_ref()
            .and_then(|s| s.current_provider())
            .unwrap_or_else(|| self.provider.clone());
        let (action, output) = gated_result(event.presentation, &provider);
        let is_error = !event.raw_result.is_success();
        self.sink.tool_finished(ToolFinishDetails {
            name: event.tool_name,
            arguments: &arguments,
            output: &output,
            is_error,
        });

        if let Some(steering) = &self.steering {
            let messages = steering.poll_steering().await;
            if !messages.is_empty() {
                self.set_steered(true);
                let steering_text = format_steering_messages(&messages);
                let augmented = attach_steering_to_output(&output, &steering_text);
                return ToolResultAction::rewrite(augmented);
            }
        }

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
