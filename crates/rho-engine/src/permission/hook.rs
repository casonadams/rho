use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::types::InteractionResponse;
use rig::agent::hook::{AgentHook, DispatchAction, DispatchEvent, HookContext};
use rig::effect::EffectKind;
use rig::error::{ErrorKind, ErrorReport};
use serde_json::Value;

use super::eval::{ask_drafts, decide_tool_call};
use super::guard::{GuardEvaluator, JudgeEvaluator};
use super::policy::{Policy, load_policy, save_allow_rule, target_config_path};
use super::prompt::{build_permission_prompt_with_evaluator, rewrite_tool_args};
use super::suggest::{canonical_tool, match_input, suggested_rule};
use super::types::{Decision, EvalRequest, RuleDraft};

pub struct PermissionHook {
    working_dir: Option<PathBuf>,
    presenter: Arc<dyn Presenter>,
    policy: Arc<RwLock<Policy>>,
    guard_evaluator: Option<GuardEvaluator>,
    judge_evaluator: Option<JudgeEvaluator>,
}

impl PermissionHook {
    pub fn new(working_dir: Option<PathBuf>, presenter: Arc<dyn Presenter>) -> Self {
        let (policy, _) = load_policy(working_dir.as_deref());
        Self {
            working_dir,
            presenter,
            policy: Arc::new(RwLock::new(policy)),
            guard_evaluator: None,
            judge_evaluator: None,
        }
    }

    pub fn with_policy(working_dir: Option<PathBuf>, presenter: Arc<dyn Presenter>, policy: Policy) -> Self {
        Self {
            working_dir,
            presenter,
            policy: Arc::new(RwLock::new(policy)),
            guard_evaluator: None,
            judge_evaluator: None,
        }
    }

    pub fn with_guard(mut self, guard: GuardEvaluator) -> Self {
        self.guard_evaluator = Some(guard);
        self
    }

    pub fn with_judge(mut self, judge: JudgeEvaluator) -> Self {
        self.judge_evaluator = Some(judge);
        self
    }

    async fn map_interaction_action(
        &self,
        response: Option<InteractionResponse>,
        req: EvalRequest<'_>,
        drafts: &[RuleDraft],
    ) -> DispatchAction {
        match response {
            Some(InteractionResponse::Selected(0 | 1)) => DispatchAction::Proceed,
            Some(InteractionResponse::SelectedWithInput { index: 1, text }) => {
                let patched_value = rewrite_tool_args(req.args, &text);
                let args_str = serde_json::to_string(&patched_value).unwrap_or_default();
                DispatchAction::Patch(EffectKind::ToolCall {
                    name: req.tool.to_string(),
                    args: args_str,
                })
            }
            Some(InteractionResponse::Selected(2)) => {
                self.apply_always_allow(req, drafts, None).await;
                DispatchAction::Proceed
            }
            Some(InteractionResponse::SelectedWithInput { index: 2, text }) => {
                self.apply_always_allow(req, drafts, Some(&text)).await;
                DispatchAction::Proceed
            }
            Some(InteractionResponse::SelectedWithInput { index: 3, text })
            | Some(InteractionResponse::Custom(text)) => skip_with_feedback(&text),
            _ => DispatchAction::Deny(ErrorReport::new(ErrorKind::Other, "Operation denied by user.")),
        }
    }

    async fn handle_ask(
        &self,
        req: EvalRequest<'_>,
        drafts: &[RuleDraft],
        notice: Option<&str>,
        risk: Option<&str>,
    ) -> DispatchAction {
        self.handle_ask_with_evaluator(req, drafts, notice, risk, None).await
    }

    async fn handle_ask_with_evaluator(
        &self,
        req: EvalRequest<'_>,
        drafts: &[RuleDraft],
        notice: Option<&str>,
        risk: Option<&str>,
        evaluator: Option<&str>,
    ) -> DispatchAction {
        if !self.presenter.has_interactive_ui() {
            let detail = notice.or(risk).map(|r| format!(": {r}")).unwrap_or_default();
            return DispatchAction::Deny(ErrorReport::new(
                ErrorKind::Other,
                format!(
                    "Permission required for tool '{}'{detail} but cannot prompt in headless mode",
                    req.tool
                ),
            ));
        }

        let prompt = build_permission_prompt_with_evaluator(req.tool, req.args, drafts, notice, risk, evaluator);
        let response = self.presenter.request_interaction(prompt).await;
        self.map_interaction_action(response, req, drafts).await
    }

    async fn evaluate_guard_or_ask(&self, req: EvalRequest<'_>, drafts: &[RuleDraft]) -> DispatchAction {
        if req.tool == "bash" {
            let cmd = match_input(req.args);
            if let Some(judge) = &self.judge_evaluator {
                match judge.evaluate_bash_safety(&cmd).await {
                    Ok(true) => return DispatchAction::Proceed,
                    Ok(false) => {
                        return self
                            .handle_ask_with_evaluator(
                                req,
                                drafts,
                                Some("Flagged by Judge decision model"),
                                None,
                                Some("Judge"),
                            )
                            .await;
                    }
                    Err(_) => {
                        // Fall back to generative guard model if judge times out or is unreachable
                    }
                }
            }

            if let Some(guard) = &self.guard_evaluator {
                let verdict = guard.evaluate(&cmd).await;
                if verdict.safe {
                    return DispatchAction::Proceed;
                }
                let (notice, risk) = match &verdict.action {
                    Some(action) => (Some(action.as_str()), Some(verdict.reason.as_str())),
                    None => (Some(verdict.reason.as_str()), None),
                };
                return self
                    .handle_ask_with_evaluator(req, drafts, notice, risk, Some("Guard"))
                    .await;
            }
        }
        self.handle_ask(req, drafts, None, None).await
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

fn skip_with_feedback(text: &str) -> DispatchAction {
    let trimmed = text.trim();
    let msg = if trimmed.is_empty() {
        "Operation denied by user.".to_string()
    } else {
        format!("Operation denied by user: {trimmed}")
    };
    DispatchAction::Deny(ErrorReport::new(ErrorKind::Other, msg))
}

impl AgentHook for PermissionHook {
    async fn on_dispatch(&self, _ctx: &HookContext, event: DispatchEvent<'_>) -> DispatchAction {
        let (tool_name, args_str) = match event.kind {
            EffectKind::ToolCall { name, args } => (name.as_str(), args.as_str()),
            _ => return DispatchAction::Proceed,
        };
        let arguments = serde_json::from_str(args_str).unwrap_or(Value::Null);
        let policy = self.policy.read().await.clone();
        let req = EvalRequest {
            tool: tool_name,
            args: &arguments,
            working_dir: self.working_dir.as_deref(),
        };

        match decide_tool_call(&policy, req) {
            Decision::Allow => DispatchAction::Proceed,
            Decision::Deny(reason) => DispatchAction::Deny(ErrorReport::new(ErrorKind::Other, reason)),
            Decision::Ask => {
                let drafts = ask_drafts(&policy, req);
                self.evaluate_guard_or_ask(req, &drafts).await
            }
        }
    }
}
