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

    async fn map_interaction_action(
        &self,
        response: Option<InteractionResponse>,
        req: EvalRequest<'_>,
        drafts: &[RuleDraft],
    ) -> ToolCallAction {
        match response {
            Some(InteractionResponse::Selected(0 | 1)) => ToolCallAction::run(),
            Some(InteractionResponse::SelectedWithInput { index: 1, text }) => {
                ToolCallAction::rewrite(rewrite_tool_args(req.args, &text))
            }
            Some(InteractionResponse::Selected(2)) => {
                self.apply_always_allow(req, drafts, None).await;
                ToolCallAction::run()
            }
            Some(InteractionResponse::SelectedWithInput { index: 2, text }) => {
                self.apply_always_allow(req, drafts, Some(&text)).await;
                ToolCallAction::run()
            }
            Some(InteractionResponse::SelectedWithInput { index: 3, text })
            | Some(InteractionResponse::Custom(text)) => skip_with_feedback(&text),
            _ => ToolCallAction::skip("Operation denied by user."),
        }
    }

    async fn handle_ask(&self, req: EvalRequest<'_>, drafts: &[RuleDraft]) -> ToolCallAction {
        if !self.presenter.has_interactive_ui() {
            return ToolCallAction::skip(format!(
                "Permission required for tool '{}' but cannot prompt in headless mode",
                req.tool
            ));
        }

        let prompt = build_permission_prompt(req.tool, req.args, drafts);
        let response = self.presenter.request_interaction(prompt).await;
        self.map_interaction_action(response, req, drafts).await
    }

    async fn apply_always_allow(&self, req: EvalRequest<'_>, drafts: &[RuleDraft], custom_pattern: Option<&str>) {
        if let Some(target) = target_config_path(req.working_dir) {
            if let Some(pattern) = custom_pattern.map(str::trim).filter(|p| !p.is_empty()) {
                save_custom_pattern(&target, req.tool, drafts, pattern);
            } else {
                save_drafts_or_fallback(&target, req.tool, req.args, drafts);
            }
        }
        let (new_policy, _) = load_policy(req.working_dir);
        let mut guard = self.policy.write().await;
        *guard = new_policy;
    }
}

fn save_custom_pattern(target: &std::path::Path, tool: &str, drafts: &[RuleDraft], pattern: &str) {
    let surface = drafts
        .first()
        .map(|d| d.surface.as_str())
        .unwrap_or_else(|| canonical_tool(tool));
    let _ = save_allow_rule(target, surface, pattern);
}

fn save_drafts_or_fallback(target: &std::path::Path, tool: &str, args: &Value, drafts: &[RuleDraft]) {
    if drafts.is_empty() {
        let rule = suggested_rule(tool, &match_input(args));
        let _ = save_allow_rule(target, canonical_tool(tool), &rule);
    } else {
        for draft in drafts {
            let _ = save_allow_rule(target, &draft.surface, &draft.pattern);
        }
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
