use super::events::*;
use super::process::{DaemonProcess, DaemonSpawnArgs};
use crate::plugin::host::HostDispatcher;
use crate::plugin::protocol::{
    PluginFlow, flow_to_completion_call_action, flow_to_invalid_tool_call_action, flow_to_observation_action,
    flow_to_tool_call_action,
};
use rho_harness_core::config::PluginConfig;
use rho_harness_core::presentation::presenter::Presenter;
use rig::agent::hook::{
    AgentHook, CompletionCall, CompletionCallAction, CompletionResponse, HookContext, InvalidToolCallAction,
    InvalidToolCallContext, ObservationAction, ToolCall, ToolCallAction, ToolResultAction, ToolResultEvent,
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

pub struct DaemonHook {
    daemons: Vec<Arc<DaemonProcess>>,
}

impl DaemonHook {
    pub async fn new(
        plugins: &BTreeMap<String, PluginConfig>,
        working_dir: &Path,
        presenter: Arc<dyn Presenter>,
    ) -> Self {
        let dispatcher = Arc::new(HostDispatcher::new(presenter));
        let mut daemons = Vec::new();

        for (name, config) in plugins {
            if !config.enabled || (config.path.as_os_str().is_empty() && config.command.is_none()) {
                continue;
            }
            let spawn_args = DaemonSpawnArgs {
                name,
                config,
                working_dir,
                dispatcher: dispatcher.clone(),
            };
            if let Ok(process) = DaemonProcess::spawn(spawn_args).await {
                daemons.push(Arc::new(process));
            }
        }
        Self { daemons }
    }

    pub fn from_daemons(daemons: Vec<Arc<DaemonProcess>>) -> Self {
        Self { daemons }
    }

    pub fn is_empty(&self) -> bool {
        self.daemons.is_empty()
    }
}

impl AgentHook for DaemonHook {
    async fn on_tool_call(&self, _ctx: &HookContext, event: ToolCall<'_>) -> ToolCallAction {
        let val = tool_call_event(event);
        for daemon in &self.daemons {
            if !daemon.subscribes_to("tool_call") {
                continue;
            }
            match daemon.call("hook/tool_call", val.clone()).await {
                Ok(res) => {
                    if let Some(flow) = res.result.and_then(|r| serde_json::from_value::<PluginFlow>(r).ok()) {
                        let action = flow_to_tool_call_action(flow);
                        if action != ToolCallAction::run() {
                            return action;
                        }
                    }
                }
                Err(err) => {
                    return ToolCallAction::skip(format!("Plugin '{}' failed: {err}", daemon.name));
                }
            }
        }
        ToolCallAction::run()
    }

    async fn on_tool_result(&self, _ctx: &HookContext, event: ToolResultEvent<'_>) -> ToolResultAction {
        let val = tool_result_event(event);
        for daemon in &self.daemons {
            if daemon.subscribes_to("tool_result") {
                let _ = daemon.call("hook/tool_result", val.clone()).await;
            }
        }
        ToolResultAction::keep()
    }

    async fn on_invalid_tool_call(
        &self,
        _ctx: &HookContext,
        event: &InvalidToolCallContext,
    ) -> Option<InvalidToolCallAction> {
        let val = invalid_tool_call_event(event);
        for daemon in &self.daemons {
            if !daemon.subscribes_to("invalid_tool_call") {
                continue;
            }
            if let Ok(res) = daemon.call("hook/invalid_tool_call", val.clone()).await
                && let Some(flow) = res.result.and_then(|r| serde_json::from_value::<PluginFlow>(r).ok())
            {
                let action = flow_to_invalid_tool_call_action(flow);
                if action != InvalidToolCallAction::Fail {
                    return Some(action);
                }
            }
        }
        None
    }

    async fn on_completion_call(&self, _ctx: &HookContext, event: CompletionCall<'_>) -> CompletionCallAction {
        let val = completion_call_event(event);
        for daemon in &self.daemons {
            if !daemon.subscribes_to("completion_call") {
                continue;
            }
            if let Ok(res) = daemon.call("hook/completion_call", val.clone()).await
                && let Some(flow) = res.result.and_then(|r| serde_json::from_value::<PluginFlow>(r).ok())
            {
                let action = flow_to_completion_call_action(flow);
                if action != CompletionCallAction::continue_run() {
                    return action;
                }
            }
        }
        CompletionCallAction::continue_run()
    }

    async fn on_completion_response(&self, _ctx: &HookContext, event: CompletionResponse<'_>) -> ObservationAction {
        let val = completion_response_event(event);
        for daemon in &self.daemons {
            if !daemon.subscribes_to("completion_response") {
                continue;
            }
            if let Ok(res) = daemon.call("hook/completion_response", val.clone()).await
                && let Some(flow) = res.result.and_then(|r| serde_json::from_value::<PluginFlow>(r).ok())
            {
                let action = flow_to_observation_action(flow);
                if action != ObservationAction::continue_run() {
                    return action;
                }
            }
        }
        ObservationAction::continue_run()
    }
}
