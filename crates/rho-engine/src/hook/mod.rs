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
    Stop(String),
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
    hook_dirs: Vec<PathBuf>,
}

impl LifecycleHook {
    pub fn new(working_dir: PathBuf, session_id: impl Into<String>, presenter: Arc<dyn Presenter>) -> Self {
        Self {
            working_dir,
            session_id: session_id.into(),
            turn: AtomicUsize::new(1),
            presenter,
            hook_dirs: Vec::new(),
        }
    }

    pub fn with_hook_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.hook_dirs.extend(dirs);
        self
    }

    pub fn presenter(&self) -> &Arc<dyn Presenter> {
        &self.presenter
    }

    pub fn find_all_hooks(&self, name: &str) -> Vec<PathBuf> {
        let home = rho_harness_core::config::dirs_fallback();
        let mut roots = vec![home.as_path()];
        if crate::mcp::trust::project_trusted_by_env() {
            roots.push(self.working_dir.as_path());
        }
        collect_hooks(name, &self.hook_dirs, &roots)
    }

    pub fn find_hook(&self, name: &str) -> Option<PathBuf> {
        self.find_all_hooks(name).into_iter().next()
    }

    async fn run_hooks_for_event(&self, name: &str, event: &HookEvent) {
        for hook in self.find_all_hooks(name) {
            let _ = run_hook(&hook, event, &self.working_dir, DEFAULT_HOOK_TIMEOUT).await;
        }
    }

    pub async fn notify_turn_start(&self, prompt: &str) {
        let event = HookEvent::TurnStart {
            prompt: prompt.to_string(),
            turn: self.turn.load(Ordering::SeqCst),
            session_id: self.session_id.clone(),
        };
        self.run_hooks_for_event("turn_start", &event).await;
    }

    pub async fn notify_turn_end(&self, status: &str, tool_calls_count: usize) {
        let event = HookEvent::TurnEnd {
            status: status.to_string(),
            tool_calls_count,
            turn: self.turn.fetch_add(1, Ordering::SeqCst),
            session_id: self.session_id.clone(),
        };
        self.run_hooks_for_event("turn_end", &event).await;
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
            Ok(HookAction::Stop { reason }) => ToolDispatchAction::Stop(reason),
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

fn collect_hooks(name: &str, hook_dirs: &[PathBuf], roots: &[&Path]) -> Vec<PathBuf> {
    let dirs = hook_dirs.iter().cloned().chain(
        roots
            .iter()
            .flat_map(|root| [root.join(".agents").join("hooks"), root.join(".claude").join("hooks")]),
    );
    let mut hooks = Vec::new();
    for hook in dirs.filter_map(|dir| find_hook_in_dir(&dir, name)) {
        if !hooks.contains(&hook) {
            hooks.push(hook);
        }
    }
    hooks
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
