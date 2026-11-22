use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::types::InteractionResponse;
use serde_json::Value;

use super::eval::{ask_drafts, decide_tool_call, runs_shell_command};
use super::guard::GuardEvaluator;
use super::policy::{Policy, load_policy, save_allow_rule, target_config_path};
use super::prompt::{build_permission_prompt_with_evaluator, rewrite_tool_args};
use super::suggest::{match_input, suggested_rule};
use super::types::{Decision, EvalRequest, RuleDraft};

#[derive(Debug, PartialEq, Eq)]
pub enum PermissionAction {
    Proceed,
    Deny(String),
    RewriteArgs(Value),
}

pub struct PermissionHook {
    working_dir: Option<PathBuf>,
    /// The project whose policy files are reloaded after a rule is saved; differs from
    /// `working_dir` for agents running in a worktree.
    policy_dir: Option<PathBuf>,
    presenter: Arc<dyn Presenter>,
    policy: Arc<RwLock<Policy>>,
    guard_evaluator: Option<GuardEvaluator>,
    mcp_servers: Vec<String>,
    prompt_lock: Arc<tokio::sync::Mutex<()>>,
    /// The subagent whose tool calls this copy of the hook checks; its prompts mark it as waiting.
    asker: Option<rho_harness_core::agents::AgentHandle>,
}

fn settled(decision: Decision) -> Option<PermissionAction> {
    match decision {
        Decision::Allow => Some(PermissionAction::Proceed),
        Decision::Deny(reason) => Some(PermissionAction::Deny(reason)),
        Decision::Ask => None,
    }
}

impl PermissionHook {
    pub fn new(working_dir: Option<PathBuf>, presenter: Arc<dyn Presenter>) -> Self {
        let policy = load_policy(working_dir.as_deref());
        Self {
            policy_dir: working_dir.clone(),
            working_dir,
            presenter,
            policy: Arc::new(RwLock::new(policy)),
            guard_evaluator: None,
            mcp_servers: Vec::new(),
            prompt_lock: Arc::default(),
            asker: None,
        }
    }

    #[cfg(test)]
    pub fn with_policy(working_dir: Option<PathBuf>, presenter: Arc<dyn Presenter>, policy: Policy) -> Self {
        Self {
            policy_dir: working_dir.clone(),
            working_dir,
            presenter,
            policy: Arc::new(RwLock::new(policy)),
            guard_evaluator: None,
            mcp_servers: Vec::new(),
            prompt_lock: Arc::default(),
            asker: None,
        }
    }

    pub fn with_guard(mut self, guard: GuardEvaluator) -> Self {
        self.guard_evaluator = Some(guard);
        self
    }

    pub fn with_mcp_servers(mut self, servers: Vec<String>) -> Self {
        self.mcp_servers = servers;
        self
    }

    /// The same rules, approvals, and prompt queue, with paths judged relative to `working_dir`
    /// (an agent's worktree), so its own files count as inside the workspace.
    #[must_use]
    pub fn for_working_dir(&self, working_dir: PathBuf) -> Self {
        Self {
            working_dir: Some(working_dir),
            policy_dir: self.policy_dir.clone(),
            presenter: self.presenter.clone(),
            policy: self.policy.clone(),
            guard_evaluator: self.guard_evaluator.clone(),
            mcp_servers: self.mcp_servers.clone(),
            prompt_lock: self.prompt_lock.clone(),
            asker: self.asker.clone(),
        }
    }

    /// The same hook for subagent `asker`: while one of its prompts is open, its monitor record says
    /// it is waiting for approval, so the interface can show or switch to it.
    #[must_use]
    pub fn for_agent(&self, asker: rho_harness_core::agents::AgentHandle) -> Self {
        Self {
            working_dir: self.working_dir.clone(),
            policy_dir: self.policy_dir.clone(),
            presenter: self.presenter.clone(),
            policy: self.policy.clone(),
            guard_evaluator: self.guard_evaluator.clone(),
            mcp_servers: self.mcp_servers.clone(),
            prompt_lock: self.prompt_lock.clone(),
            asker: Some(asker),
        }
    }

    async fn map_interaction_action(
        &self,
        response: Option<InteractionResponse>,
        req: EvalRequest<'_>,
        drafts: &[RuleDraft],
    ) -> PermissionAction {
        match response {
            Some(InteractionResponse::Selected(0 | 1)) => PermissionAction::Proceed,
            Some(InteractionResponse::SelectedWithInput { index: 1, text }) => {
                let patched_value = rewrite_tool_args(req.args, &text);
                PermissionAction::RewriteArgs(patched_value)
            }
            Some(InteractionResponse::Selected(2)) => self.always_allow(req, drafts, None).await,
            Some(InteractionResponse::SelectedWithInput { index: 2, text }) => {
                self.always_allow(req, drafts, Some(&text)).await
            }
            Some(InteractionResponse::SelectedWithInput { index: 3, text })
            | Some(InteractionResponse::Custom(text)) => skip_with_feedback(&text),
            _ => PermissionAction::Deny("Operation cancelled by user.".to_string()),
        }
    }

    async fn handle_ask(
        &self,
        req: EvalRequest<'_>,
        drafts: &[RuleDraft],
        notice: Option<&str>,
        risk: Option<&str>,
    ) -> PermissionAction {
        self.handle_ask_with_evaluator(req, drafts, notice, risk, None).await
    }

    async fn handle_ask_with_evaluator(
        &self,
        req: EvalRequest<'_>,
        drafts: &[RuleDraft],
        notice: Option<&str>,
        risk: Option<&str>,
        evaluator: Option<&str>,
    ) -> PermissionAction {
        if !self.presenter.has_interactive_ui() {
            let detail = notice.or(risk).map(|r| format!(": {r}")).unwrap_or_default();
            return PermissionAction::Deny(format!(
                "Permission required for tool '{}'{detail} but cannot prompt in headless mode",
                req.tool
            ));
        }

        let prompt = build_permission_prompt_with_evaluator(req.tool, req.args, drafts, notice, risk, evaluator);
        let _waiting = self
            .asker
            .as_ref()
            .map(rho_harness_core::agents::AgentHandle::awaiting_approval);
        let response = self.presenter.request_interaction(prompt).await;
        self.map_interaction_action(response, req, drafts).await
    }

    async fn evaluate_guard_or_ask(&self, req: EvalRequest<'_>, drafts: &[RuleDraft]) -> PermissionAction {
        if runs_shell_command(req.tool, req.args) {
            let cmd = match_input(req.args);
            if let Some(guard) = &self.guard_evaluator {
                let verdict = guard.evaluate(&cmd).await;
                if verdict.safe {
                    return PermissionAction::Proceed;
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

    async fn always_allow(
        &self,
        req: EvalRequest<'_>,
        drafts: &[RuleDraft],
        custom_pattern: Option<&str>,
    ) -> PermissionAction {
        match self.save_always_allow(req, drafts, custom_pattern).await {
            Ok(()) => PermissionAction::Proceed,
            Err(error) => {
                PermissionAction::Deny(format!("Failed to save always-allow rule: {error}. Operation not run."))
            }
        }
    }

    async fn save_always_allow(
        &self,
        req: EvalRequest<'_>,
        drafts: &[RuleDraft],
        custom_pattern: Option<&str>,
    ) -> Result<(), String> {
        let target = target_config_path().ok_or("no configuration directory is available")?;
        match custom_pattern.map(str::trim).filter(|p| !p.is_empty()) {
            Some(pattern) => save_custom_pattern(&target, req.tool, drafts, pattern)?,
            None => save_drafts_or_fallback(&target, req.tool, req.args, drafts)?,
        }
        let new_policy = load_policy(self.policy_dir.as_deref());
        let mut guard = self.policy.write().await;
        *guard = new_policy;
        Ok(())
    }

    pub async fn check_permission(&self, tool_name: &str, arguments: &Value) -> PermissionAction {
        let req = EvalRequest {
            tool: tool_name,
            args: arguments,
            working_dir: self.working_dir.as_deref(),
            mcp_servers: &self.mcp_servers,
        };
        let policy = self.policy.read().await.clone();
        match settled(decide_tool_call(&policy, req)) {
            Some(action) => action,
            None => self.ask_exclusively(req).await,
        }
    }

    /// Concurrent callers queue here so only one prompt is on screen; the policy is re-read because an earlier answer may have saved an allow rule.
    async fn ask_exclusively(&self, req: EvalRequest<'_>) -> PermissionAction {
        let _prompting = self.prompt_lock.lock().await;
        let policy = self.policy.read().await.clone();
        match settled(decide_tool_call(&policy, req)) {
            Some(action) => action,
            None => {
                let drafts = ask_drafts(&policy, req);
                self.evaluate_guard_or_ask(req, &drafts).await
            }
        }
    }
}

fn save_custom_pattern(target: &Path, tool: &str, drafts: &[RuleDraft], pattern: &str) -> Result<(), String> {
    let surface = drafts.first().map_or(tool, |d| d.surface.as_str());
    save_allow_rule(target, surface, pattern)
}

fn save_drafts_or_fallback(target: &Path, tool: &str, args: &Value, drafts: &[RuleDraft]) -> Result<(), String> {
    if drafts.is_empty() {
        let rule = suggested_rule(tool, &match_input(args));
        save_allow_rule(target, tool, &rule)
    } else {
        drafts
            .iter()
            .try_for_each(|draft| save_allow_rule(target, &draft.surface, &draft.pattern))
    }
}

fn skip_with_feedback(text: &str) -> PermissionAction {
    let trimmed = text.trim();
    let msg = if trimmed.is_empty() {
        "Operation denied by user.".to_string()
    } else {
        format!("Operation denied by user: {trimmed}")
    };
    PermissionAction::Deny(msg)
}
