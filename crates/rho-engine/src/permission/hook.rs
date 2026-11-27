use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use rho_harness_core::presentation::presenter::Presenter;
use rho_harness_core::presentation::types::InteractionResponse;
use serde_json::Value;

use super::eval::{ask_drafts, decide_tool_call};
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
    guard_all_tools: bool,
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

fn report_policy_warnings(presenter: &dyn Presenter, warnings: &[String]) {
    for warning in warnings {
        presenter.print_notice(warning);
    }
}

impl PermissionHook {
    pub fn new(working_dir: Option<PathBuf>, presenter: Arc<dyn Presenter>) -> Self {
        let loaded = load_policy(working_dir.as_deref());
        report_policy_warnings(presenter.as_ref(), &loaded.warnings);
        Self {
            policy_dir: working_dir.clone(),
            working_dir,
            presenter,
            policy: Arc::new(RwLock::new(loaded.policy)),
            guard_evaluator: None,
            guard_all_tools: false,
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
            guard_all_tools: false,
            mcp_servers: Vec::new(),
            prompt_lock: Arc::default(),
            asker: None,
        }
    }

    pub fn with_guard(mut self, guard: GuardEvaluator) -> Self {
        self.guard_evaluator = Some(guard);
        self
    }

    pub fn with_guard_all_tools(mut self, enabled: bool) -> Self {
        self.guard_all_tools = enabled;
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
            guard_all_tools: self.guard_all_tools,
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
            guard_all_tools: self.guard_all_tools,
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
    fn format_guard_input(&self, req: EvalRequest<'_>) -> (String, String) {
        if req.tool == "bash" {
            return ("bash".to_string(), match_input(req.args));
        }
        let is_mcp = req.tool == "mcp"
            || self
                .mcp_servers
                .iter()
                .any(|s| req.tool.starts_with(s.as_str()) && req.tool.as_bytes().get(s.len()) == Some(&b'_'));
        let lang = if is_mcp { "mcp" } else { req.tool }.to_string();
        let args_str = serde_json::to_string(req.args).unwrap_or_else(|_| match_input(req.args));
        (lang, format!("{} {}", req.tool, args_str))
    }

    async fn evaluate_guard_or_ask(&self, req: EvalRequest<'_>, drafts: &[RuleDraft]) -> PermissionAction {
        let Some(guard) = &self.guard_evaluator else {
            return self.handle_ask(req, drafts, None, None).await;
        };
        if req.tool != "bash" && !self.guard_all_tools {
            return self.handle_ask(req, drafts, None, None).await;
        }
        let (lang, input) = self.format_guard_input(req);
        let eval = guard.evaluate_tool(&lang, &input, req.working_dir).await;
        if eval.verdict.safe {
            eval.record_audit(super::guard::DecisionTargetKey::SafeAutoAllow, eval.source, None);
            return PermissionAction::Proceed;
        }
        let (notice, risk) = match &eval.verdict.action {
            Some(action) => (Some(action.as_str()), Some(eval.verdict.reason.as_str())),
            None => (Some(eval.verdict.reason.as_str()), None),
        };
        let action = self
            .handle_ask_with_evaluator(req, drafts, notice, risk, Some("Guard"))
            .await;
        record_post_prompt_audit(&eval, &action);
        action
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
        let loaded = load_policy(self.policy_dir.as_deref());
        report_policy_warnings(self.presenter.as_ref(), &loaded.warnings);
        let mut guard = self.policy.write().await;
        *guard = loaded.policy;
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

fn record_post_prompt_audit(eval: &super::guard::GuardEvaluation, action: &PermissionAction) {
    use super::guard::{DecisionSource, DecisionTargetKey};
    match action {
        PermissionAction::Proceed | PermissionAction::RewriteArgs(_) => {
            eval.record_audit(DecisionTargetKey::AskConfirmation, DecisionSource::UserProceed, None);
        }
        PermissionAction::Deny(reason) => {
            let (source, feedback) = if reason.contains("non-interactive") || reason.contains("headless") {
                (DecisionSource::HeadlessBlock, None)
            } else if let Some(fb) = reason.strip_prefix("Operation denied by user: ") {
                (DecisionSource::UserFeedback, Some(fb.to_string()))
            } else {
                (DecisionSource::UserCancel, None)
            };
            eval.record_audit(DecisionTargetKey::HardDeny, source, feedback);
        }
    }
}
