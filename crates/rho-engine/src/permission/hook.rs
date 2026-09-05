use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::types::InteractionResponse;
use rig::agent::hook::{AgentHook, HookContext, ToolCall, ToolCallAction};
use serde_json::Value;

use super::eval::{ask_drafts, decide_tool_call};
use super::policy::{Policy, load_policy, save_allow_rule, target_config_path};
use super::prompt::{build_permission_prompt, rewrite_tool_args};
use super::suggest::{canonical_tool, match_input, suggested_rule};
use super::types::{Decision, EvalRequest, RuleDraft};
use crate::plugin::host::HeadlessGuard;

pub struct PermissionHook {
    working_dir: Option<PathBuf>,
    presenter: Arc<dyn Presenter>,
    policy: Arc<RwLock<Policy>>,
}

impl PermissionHook {
    pub fn new(working_dir: Option<PathBuf>, presenter: Arc<dyn Presenter>) -> Self {
        let (policy, _) = load_policy(working_dir.as_deref());
        Self {
            working_dir,
            presenter,
            policy: Arc::new(RwLock::new(policy)),
        }
    }

    pub fn with_policy(working_dir: Option<PathBuf>, presenter: Arc<dyn Presenter>, policy: Policy) -> Self {
        Self {
            working_dir,
            presenter,
            policy: Arc::new(RwLock::new(policy)),
        }
    }

    async fn handle_ask(&self, req: EvalRequest<'_>, drafts: &[RuleDraft]) -> ToolCallAction {
        if HeadlessGuard::is_headless(self.presenter.as_ref()) {
            return ToolCallAction::skip(format!(
                "Permission required for tool '{}' but cannot prompt in headless mode",
                req.tool
            ));
        }

        let prompt = build_permission_prompt(req.tool, req.args, drafts);
        let response = self.presenter.request_interaction(prompt).await;

        match response {
            Some(InteractionResponse::Selected(0)) => ToolCallAction::run(),
            Some(InteractionResponse::SelectedWithInput { index: 1, text }) => {
                let new_args = rewrite_tool_args(req.args, &text);
                ToolCallAction::rewrite(new_args)
            }
            Some(InteractionResponse::Selected(1)) => ToolCallAction::run(),
            Some(InteractionResponse::Selected(2)) | Some(InteractionResponse::SelectedWithInput { index: 2, .. }) => {
                self.apply_always_allow(req, drafts).await;
                ToolCallAction::run()
            }
            Some(InteractionResponse::Selected(3)) => ToolCallAction::skip("Operation denied by user."),
            Some(InteractionResponse::SelectedWithInput { index: 3, text }) => skip_with_feedback(&text),
            Some(InteractionResponse::Custom(text)) => skip_with_feedback(&text),
            Some(InteractionResponse::Cancelled) | None => ToolCallAction::skip("Operation denied by user."),
            _ => ToolCallAction::skip("Operation denied by user."),
        }
    }

    async fn apply_always_allow(&self, req: EvalRequest<'_>, drafts: &[RuleDraft]) {
        let target_path = target_config_path(req.working_dir);
        if let Some(target) = target_path {
            if !drafts.is_empty() {
                for draft in drafts {
                    let _ = save_allow_rule(&target, &draft.surface, &draft.pattern);
                }
            } else {
                let input = match_input(req.args);
                let rule = suggested_rule(req.tool, &input);
                let _ = save_allow_rule(&target, canonical_tool(req.tool), &rule);
            }
        }
        let (new_policy, _) = load_policy(req.working_dir);
        let mut guard = self.policy.write().await;
        *guard = new_policy;
    }
}

fn skip_with_feedback(text: &str) -> ToolCallAction {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        ToolCallAction::skip("Operation denied by user.")
    } else {
        ToolCallAction::skip(format!("Operation denied by user: {trimmed}"))
    }
}

impl AgentHook for PermissionHook {
    async fn on_tool_call(&self, _ctx: &HookContext, event: ToolCall<'_>) -> ToolCallAction {
        let arguments = serde_json::from_str(event.args).unwrap_or(Value::Null);
        let policy = self.policy.read().await.clone();
        let req = EvalRequest {
            tool: event.tool_name,
            args: &arguments,
            working_dir: self.working_dir.as_deref(),
        };

        match decide_tool_call(&policy, req) {
            Decision::Allow => ToolCallAction::run(),
            Decision::Deny(reason) => ToolCallAction::skip(reason),
            Decision::Ask => {
                let drafts = ask_drafts(&policy, req);
                self.handle_ask(req, &drafts).await
            }
        }
    }
}
