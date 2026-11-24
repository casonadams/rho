mod agent_step;
mod command;
mod executor;
mod files;
mod report;
#[cfg(test)]
mod tests;
mod write_step;

use crate::tools::engine_tool::TurnToolContext;
use crate::tools::subagent::SubagentTool;
use executor::{Executor, Frame};
use report::RunLog;
use rho_harness_core::workflow::{AgentNode, ModelRole, Node, NodeKind, WorkflowBundle, WorkflowCatalog};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRun {
    pub ok: bool,
    pub report: String,
    /// The workflow started running; a run rejected before its first step has nothing to hand off.
    pub started: bool,
}

/// Runs workflows started with `/workflow`. Workflows are trusted: no permission checks or hooks apply.
#[derive(Clone)]
pub struct WorkflowRunner {
    subagents: SubagentTool,
    base_dir: PathBuf,
    home_dir: Option<PathBuf>,
}

fn collect_agents<'a>(nodes: &'a [Node], into: &mut Vec<&'a AgentNode>) {
    for node in nodes {
        if let NodeKind::Agent(agent) = &node.kind {
            into.push(agent);
        }
        for body in node.kind.bodies() {
            collect_agents(body, into);
        }
    }
}

fn bundle_agents(bundle: &WorkflowBundle) -> Vec<&AgentNode> {
    let mut agents = Vec::new();
    for wf in std::iter::once(&bundle.entry).chain(bundle.workflows.values()) {
        collect_agents(&wf.body, &mut agents);
    }
    agents
}

/// Problems that would stop the workflow at an agent step, found before any step runs.
fn preflight(name: &str, bundle: &WorkflowBundle, subagents: &SubagentTool) -> Result<(), String> {
    let agents = bundle_agents(bundle);
    let mut missing: Vec<&'static str> = agents
        .iter()
        .filter_map(|agent| agent.model.as_deref().and_then(|m| ModelRole::parse(m).ok()))
        .filter(|role| !subagents.role_configured(*role))
        .map(ModelRole::as_str)
        .collect();
    missing.sort_unstable();
    missing.dedup();
    if !missing.is_empty() {
        return Err(format!(
            "workflow '{name}' uses model roles that are not configured: {} (set models.<role> in config.toml)",
            missing.join(", ")
        ));
    }
    agents
        .iter()
        .filter_map(|agent| agent.persona.as_deref())
        .try_for_each(|persona| subagents.check_persona(persona))
        .map_err(|e| format!("workflow '{name}': {e}"))
}

impl WorkflowRunner {
    pub(crate) fn worktrees(&self) -> Option<&crate::worktree::WorktreeStore> {
        self.subagents.worktrees()
    }
    #[must_use]
    pub fn new(subagents: SubagentTool, base_dir: impl Into<PathBuf>, home_dir: Option<PathBuf>) -> Self {
        Self {
            subagents,
            base_dir: base_dir.into(),
            home_dir,
        }
    }

    /// Binds the workflow's agents to `ctx`: its cancellation, progress, monitor, session, and mailbox.
    #[must_use]
    pub fn bind(&self, ctx: &TurnToolContext) -> Self {
        Self {
            subagents: self.subagents.with_turn_context(ctx),
            ..self.clone()
        }
    }

    #[must_use]
    pub fn catalog(&self) -> WorkflowCatalog {
        WorkflowCatalog::discover(Some(&self.base_dir), self.home_dir.as_deref())
    }

    pub async fn run(&self, name: &str, inputs: BTreeMap<String, String>) -> WorkflowRun {
        let runner = self.clone();
        let catalog = match tokio::task::spawn_blocking(move || runner.catalog()).await {
            Ok(catalog) => catalog,
            Err(e) => return failed(format!("workflow discovery failed: {e}")),
        };
        let Some(bundle) = catalog.find(name) else {
            return failed(unknown_workflow(name, &catalog));
        };
        let inputs = match bundle.entry.resolve_inputs(inputs) {
            Ok(inputs) => inputs,
            Err(e) => return failed(e),
        };
        if let Err(e) = preflight(name, bundle, &self.subagents) {
            return failed(e);
        }
        self.execute(&catalog, bundle, &inputs).await
    }

    async fn execute(
        &self,
        catalog: &WorkflowCatalog,
        bundle: &WorkflowBundle,
        inputs: &BTreeMap<String, String>,
    ) -> WorkflowRun {
        let log = match RunLog::create(&self.base_dir, &bundle.name) {
            Ok(log) => log,
            Err(e) => return failed(format!("workflow '{}' could not start: {e}", bundle.name)),
        };
        log.event(
            "run_started",
            json!({ "workflow": bundle.name, "location": bundle.location, "inputs": inputs }),
        );
        let owner = self
            .subagents
            .turn_context()
            .session
            .as_ref()
            .map(|s| s.session_id.clone());
        let label = format!("workflow {}", bundle.name);
        let lease = match self.subagents.worktrees() {
            Some(store) => match store.create(&self.base_dir, &label, owner.as_deref()).await {
                Ok(worktree) => worktree.map(crate::worktree::WorktreeLease::new),
                Err(e) => {
                    let report = log.report(&format!("Workflow `{}` did not start: {e}", bundle.name), None);
                    return failed(report);
                }
            },
            None => None,
        };
        let base_dir = lease
            .as_ref()
            .map_or_else(|| self.base_dir.clone(), |l| l.worktree().path.clone());
        let executor = Executor {
            subagents: &self.subagents,
            catalog,
            base_dir: &base_dir,
            project_dir: &self.base_dir,
            log: &log,
        };
        let frame = Frame {
            bundle,
            path: bundle.name.clone(),
        };
        let result = executor.run_workflow(frame, &bundle.entry, inputs).await;
        log.event("run_finished", json!({ "ok": result.is_ok() }));
        let handoff = match lease {
            Some(lease) => {
                let text = hand_off(lease.worktree()).await;
                let _ = lease.release();
                text
            }
            None => String::new(),
        };
        let (ok, report) = match result {
            Ok(outputs) => (
                true,
                log.report(&format!("Workflow `{}` completed.", bundle.name), Some(&outputs)),
            ),
            Err(e) => (
                false,
                log.report(&format!("Workflow `{}` stopped: {e}", bundle.name), None),
            ),
        };
        WorkflowRun {
            ok,
            report: format!("{report}{handoff}"),
            started: true,
        }
    }
}

/// Where the run's changes are and how the parent lands them; a run without changes leaves nothing behind.
async fn hand_off(worktree: &crate::worktree::Worktree) -> String {
    match worktree.changes().await {
        Ok((_, changes)) if changes.is_empty() => match worktree.remove().await {
            Ok(()) => "\nThe workflow made no file changes.\n".to_string(),
            Err(e) => format!("\nThe workflow made no file changes, but its worktree could not be removed: {e}\n"),
        },
        Ok((_, changes)) => format!(
            "\nThe workflow's changes are in {}. Nothing has touched your working tree: review them (worktree action diff), then merge or discard the worktree with the worktree tool. Unmerged worktrees are deleted 24 hours after they were created.\n",
            worktree.summary(&changes)
        ),
        Err(e) => format!(
            "\nThe workflow ran in worktree `{}` ({}), but its changes could not be listed: {e}\n",
            worktree.id,
            worktree.path.display()
        ),
    }
}

fn failed(report: String) -> WorkflowRun {
    WorkflowRun {
        ok: false,
        report,
        started: false,
    }
}

fn unknown_workflow(name: &str, catalog: &WorkflowCatalog) -> String {
    let names: Vec<&str> = catalog.bundles.iter().map(|b| b.name.as_str()).collect();
    let available = if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    };
    let mut message = format!("unknown workflow '{name}'; available: {available}");
    if let Some(problem) = catalog.errors.iter().find(|e| e.contains(&format!("'{name}'"))) {
        message.push_str(&format!("\n{problem}"));
    }
    message
}
