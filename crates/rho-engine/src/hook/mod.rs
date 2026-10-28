pub mod runner;
pub mod types;

#[cfg(test)]
mod tests;

pub use runner::{DEFAULT_HOOK_TIMEOUT, parse_hook_output, run_hook};
pub use types::{HookAction, HookEvent};

use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::types::{InteractionOption, InteractionPrompt, InteractionResponse, OptionLayout};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

pub enum ToolDispatchAction {
    Proceed,
    Deny(String),
    RewriteArgs(serde_json::Value),
}

pub enum ToolOutcomeAction {
    Proceed,
    Stop(String),
    RewriteResult(String),
}

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
        let user_home = rho_harness_core::config::dirs_fallback();
        let user_dir = if self.working_dir != user_home {
            Some(user_home.as_path())
        } else {
            None
        };
        find_hook_in_paths(name, &self.working_dir, user_dir)
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

    pub async fn on_tool_dispatch(&self, tool_name: &str, args: &serde_json::Value) -> ToolDispatchAction {
        let Some(hook) = self.find_hook("tool_call") else {
            return ToolDispatchAction::Proceed;
        };

        let hook_event = HookEvent::ToolCall {
            tool_name: tool_name.to_string(),
            args: args.clone(),
            turn: self.turn.load(Ordering::SeqCst),
            session_id: self.session_id.clone(),
        };

        match run_hook(&hook, &hook_event, &self.working_dir, DEFAULT_HOOK_TIMEOUT).await {
            Ok(HookAction::Continue) => ToolDispatchAction::Proceed,
            Ok(HookAction::Stop { reason }) => ToolDispatchAction::Deny(reason),
            Ok(HookAction::Skip { reason }) => ToolDispatchAction::Deny(reason),
            Ok(HookAction::RewriteArgs { args }) => ToolDispatchAction::RewriteArgs(args),
            Ok(HookAction::Ask { message }) => {
                let approved = self.prompt_user_confirmation(&message, tool_name).await;
                if approved {
                    ToolDispatchAction::Proceed
                } else {
                    ToolDispatchAction::Deny(format!("Operation denied by user: {message}"))
                }
            }
            Ok(_) => ToolDispatchAction::Proceed,
            Err(err) => ToolDispatchAction::Deny(format!("Hook error: {err}")),
        }
    }

    pub async fn on_tool_outcome(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        output: &str,
        is_error: bool,
    ) -> ToolOutcomeAction {
        let Some(hook) = self.find_hook("tool_result") else {
            return ToolOutcomeAction::Proceed;
        };

        let hook_event = HookEvent::ToolResult {
            tool_name: tool_name.to_string(),
            args: args.clone(),
            output: output.to_string(),
            is_error,
        };

        match run_hook(&hook, &hook_event, &self.working_dir, DEFAULT_HOOK_TIMEOUT).await {
            Ok(HookAction::Stop { reason }) => ToolOutcomeAction::Stop(reason),
            Ok(HookAction::RewriteResult { result }) => ToolOutcomeAction::RewriteResult(result),
            _ => ToolOutcomeAction::Proceed,
        }
    }
}

pub fn find_hook_in_paths(name: &str, project_dir: &Path, user_dir: Option<&Path>) -> Option<PathBuf> {
    let project_hooks = project_dir.join(".agents").join("hooks");
    if let Some(hook) = find_hook_in_dir(&project_hooks, name) {
        return Some(hook);
    }

    if let Some(user) = user_dir {
        let user_hooks = user.join(".agents").join("hooks");
        if let Some(hook) = find_hook_in_dir(&user_hooks, name) {
            return Some(hook);
        }
    }

    None
}

fn find_hook_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }

    let candidates = [
        dir.join(name),
        dir.join(format!("on_{name}")),
        dir.join(name.replace('_', "-")),
        dir.join(format!("on-{}", name.replace('_', "-"))),
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
