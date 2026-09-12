pub mod runner;
pub mod types;

#[cfg(test)]
mod tests;

pub use runner::{DEFAULT_HOOK_TIMEOUT, run_hook};
pub use types::{HookAction, HookEvent};

use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::types::{InteractionOption, InteractionPrompt, InteractionResponse, OptionLayout};
use rig::agent::hook::{
    AgentHook, CompletionCall, CompletionCallAction, HookContext, InvalidToolCallAction, InvalidToolCallContext,
    ToolCall, ToolCallAction, ToolResultAction, ToolResultEvent,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct LifecycleHook {
    working_dir: PathBuf,
    session_id: String,
    turn: AtomicUsize,
    presenter: Arc<dyn Presenter>,
}

impl LifecycleHook {
    pub fn new(working_dir: PathBuf, session_id: impl Into<String>, presenter: Arc<dyn Presenter>) -> Self {
        Self {
            working_dir,
            session_id: session_id.into(),
            turn: AtomicUsize::new(1),
            presenter,
        }
    }

    pub fn find_hook(&self, name: &str) -> Option<PathBuf> {
        let hooks_dir = self.working_dir.join(".rho").join("hooks");
        if !hooks_dir.is_dir() {
            return None;
        }

        let candidates = [
            hooks_dir.join(name),
            hooks_dir.join(format!("on_{name}")),
            hooks_dir.join(name.replace('_', "-")),
            hooks_dir.join(format!("on-{}", name.replace('_', "-"))),
        ];

        for candidate in candidates {
            if candidate.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = candidate.metadata()
                        && meta.permissions().mode() & 0o111 != 0
                    {
                        return Some(candidate);
                    }
                }
                #[cfg(not(unix))]
                return Some(candidate);
            }
        }

        None
    }

    pub async fn notify_turn_start(&self, prompt: &str) {
        if let Some(hook) = self.find_hook("turn_start") {
            let event = HookEvent::TurnStart {
                prompt: prompt.to_string(),
                turn: self.turn.load(Ordering::SeqCst),
                session_id: self.session_id.clone(),
            };
            let _ = run_hook(&hook, &event, &self.working_dir, DEFAULT_HOOK_TIMEOUT).await;
        }
    }

    pub async fn notify_turn_end(&self, status: &str, tool_calls_count: usize) {
        if let Some(hook) = self.find_hook("turn_end") {
            let event = HookEvent::TurnEnd {
                status: status.to_string(),
                tool_calls_count,
                turn: self.turn.fetch_add(1, Ordering::SeqCst),
                session_id: self.session_id.clone(),
            };
            let _ = run_hook(&hook, &event, &self.working_dir, DEFAULT_HOOK_TIMEOUT).await;
        }
    }

    async fn prompt_user_confirmation(&self, message: &str, tool_name: &str) -> bool {
        if !self.presenter.has_interactive_ui() {
            return false;
        }
        let prompt = InteractionPrompt {
            title: format!("Hook prompt for tool: {tool_name}"),
            body: message.to_string(),
            options: vec![
                InteractionOption {
                    label: "Allow".to_string(),
                    description: Some("Allow execution".to_string()),
                    input: None,
                },
                InteractionOption {
                    label: "Deny".to_string(),
                    description: Some("Block execution".to_string()),
                    input: None,
                },
            ],
            initial_selection: 0,
            allow_custom: false,
            initial_text: None,
            option_layout: OptionLayout::Horizontal,
        };

        matches!(
            self.presenter.request_interaction(prompt).await,
            Some(InteractionResponse::Selected(0))
        )
    }
}

impl AgentHook for LifecycleHook {
    async fn on_tool_call(&self, _ctx: &HookContext, event: ToolCall<'_>) -> ToolCallAction {
        let Some(hook) = self.find_hook("tool_call") else {
            return ToolCallAction::run();
        };

        let parsed_args = serde_json::from_str(event.args).unwrap_or(serde_json::json!(event.args));
        let hook_event = HookEvent::ToolCall {
            tool_name: event.tool_name.to_string(),
            args: parsed_args,
            turn: self.turn.load(Ordering::SeqCst),
            session_id: self.session_id.clone(),
        };

        match run_hook(&hook, &hook_event, &self.working_dir, DEFAULT_HOOK_TIMEOUT).await {
            Ok(HookAction::Continue) => ToolCallAction::run(),
            Ok(HookAction::Stop { reason }) => ToolCallAction::stop(reason),
            Ok(HookAction::Skip { reason }) => ToolCallAction::skip(reason),
            Ok(HookAction::RewriteArgs { args }) => ToolCallAction::rewrite(args),
            Ok(HookAction::Ask { message }) => {
                let approved = self.prompt_user_confirmation(&message, event.tool_name).await;
                if approved {
                    ToolCallAction::run()
                } else {
                    ToolCallAction::skip(format!("Operation denied by user: {message}"))
                }
            }
            Ok(_) => ToolCallAction::run(),
            Err(err) => ToolCallAction::skip(format!("Hook error: {err}")),
        }
    }

    async fn on_tool_result(&self, _ctx: &HookContext, event: ToolResultEvent<'_>) -> ToolResultAction {
        let Some(hook) = self.find_hook("tool_result") else {
            return ToolResultAction::keep();
        };

        let parsed_args = serde_json::from_str(event.args).unwrap_or(serde_json::json!(event.args));
        let hook_event = HookEvent::ToolResult {
            tool_name: event.tool_name.to_string(),
            args: parsed_args,
            output: event.presentation.render(),
            is_error: !event.raw_result.is_success(),
        };

        match run_hook(&hook, &hook_event, &self.working_dir, DEFAULT_HOOK_TIMEOUT).await {
            Ok(HookAction::Stop { reason }) => ToolResultAction::stop(reason),
            Ok(HookAction::RewriteResult { result }) => ToolResultAction::rewrite(result),
            _ => ToolResultAction::keep(),
        }
    }

    async fn on_completion_call(&self, _ctx: &HookContext, event: CompletionCall<'_>) -> CompletionCallAction {
        let Some(hook) = self.find_hook("completion_call") else {
            return CompletionCallAction::continue_run();
        };

        let hook_event = HookEvent::CompletionCall {
            turn: self.turn.load(Ordering::SeqCst),
            prompt: serde_json::to_value(event.prompt).unwrap_or_default(),
        };

        match run_hook(&hook, &hook_event, &self.working_dir, DEFAULT_HOOK_TIMEOUT).await {
            Ok(HookAction::Stop { reason }) => CompletionCallAction::stop(reason),
            _ => CompletionCallAction::continue_run(),
        }
    }

    async fn on_invalid_tool_call(
        &self,
        _ctx: &HookContext,
        event: &InvalidToolCallContext,
    ) -> Option<InvalidToolCallAction> {
        let hook = self.find_hook("invalid_tool_call")?;

        let parsed_args = event
            .args
            .as_deref()
            .and_then(|a| serde_json::from_str(a).ok())
            .unwrap_or(serde_json::json!(event.args));

        let hook_event = HookEvent::InvalidToolCall {
            tool_name: event.tool_name.to_string(),
            args: parsed_args,
            available_tools: event.available_tools.to_vec(),
        };

        match run_hook(&hook, &hook_event, &self.working_dir, DEFAULT_HOOK_TIMEOUT).await {
            Ok(HookAction::Stop { reason }) => Some(InvalidToolCallAction::Stop { reason }),
            Ok(HookAction::Retry { feedback }) => Some(InvalidToolCallAction::Retry { feedback }),
            Ok(HookAction::Skip { reason }) => Some(InvalidToolCallAction::Skip { reason }),
            _ => None,
        }
    }
}
