use super::types::{PluginFlow, RequestPatchPayload};
use rig::agent::hook::{
    CompletionCallAction, InvalidToolCallAction, ObservationAction, RequestPatch, ToolCallAction, ToolResultAction,
};

pub fn flow_to_tool_call_action(flow: PluginFlow) -> ToolCallAction {
    match flow {
        PluginFlow::Continue => ToolCallAction::run(),
        PluginFlow::Skip { reason } => ToolCallAction::skip(reason),
        PluginFlow::RewriteArgs { args } => ToolCallAction::rewrite(args),
        PluginFlow::Terminate { reason } => ToolCallAction::stop(reason),
        _ => ToolCallAction::run(),
    }
}

pub fn flow_to_tool_result_action(flow: PluginFlow) -> ToolResultAction {
    match flow {
        PluginFlow::Continue => ToolResultAction::keep(),
        PluginFlow::RewriteResult { result } => ToolResultAction::rewrite(result),
        PluginFlow::Terminate { reason } => ToolResultAction::stop(reason),
        _ => ToolResultAction::keep(),
    }
}

pub fn flow_to_invalid_tool_call_action(flow: PluginFlow) -> InvalidToolCallAction {
    match flow {
        PluginFlow::Continue => InvalidToolCallAction::Fail,
        PluginFlow::Repair { tool_name } => InvalidToolCallAction::Repair { tool_name },
        PluginFlow::Retry { feedback } => InvalidToolCallAction::Retry { feedback },
        PluginFlow::Skip { reason } => InvalidToolCallAction::Skip { reason },
        PluginFlow::Terminate { reason } => InvalidToolCallAction::Stop { reason },
        _ => InvalidToolCallAction::Fail,
    }
}

pub fn flow_to_completion_call_action(flow: PluginFlow) -> CompletionCallAction {
    match flow {
        PluginFlow::Continue => CompletionCallAction::continue_run(),
        PluginFlow::OverrideRequest { request } => CompletionCallAction::patch(request_patch_payload_to_rig(request)),
        PluginFlow::Terminate { reason } => CompletionCallAction::stop(reason),
        _ => CompletionCallAction::continue_run(),
    }
}

pub fn flow_to_observation_action(flow: PluginFlow) -> ObservationAction {
    match flow {
        PluginFlow::Continue => ObservationAction::continue_run(),
        PluginFlow::Terminate { reason } => ObservationAction::stop(reason),
        _ => ObservationAction::continue_run(),
    }
}

pub fn request_patch_payload_to_rig(payload: RequestPatchPayload) -> RequestPatch {
    let mut patch = RequestPatch::new();
    if let Some(preamble) = payload.preamble {
        patch = patch.preamble(preamble);
    }
    if let Some(temp) = payload.temperature {
        patch = patch.temperature(temp);
    }
    if let Some(max_tokens) = payload.max_tokens {
        patch = patch.max_tokens(max_tokens);
    }
    if let Some(tools) = payload.active_tools {
        patch = patch.active_tools(tools);
    }
    if let Some(params) = payload.additional_params {
        patch = patch.additional_params(params);
    }
    patch
}
