mod agent_step;
mod command;
mod executor;
mod files;
mod report;
#[cfg(test)]
mod tests;

use crate::engine::runner::turn::types::CancellationSignal;
use crate::tools::engine_tool::TurnToolContext;
use crate::tools::subagent::SubagentTool;
use executor::{Executor, Frame};
use report::RunLog;
use rho_harness_core::presentation::ToolStreamPort;
use rho_harness_core::workflow::{ModelRole, Node, NodeKind, WorkflowBundle, WorkflowCatalog};
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

fn collect_roles(nodes: &[Node], into: &mut Vec<ModelRole>) {
    for node in nodes {
        if let NodeKind::Agent(agent) = &node.kind
            && let Some(role) = agent.model
        {
            into.push(role);
        }
        for body in node.kind.bodies() {
            collect_roles(body, into);
        }
    }
}

fn missing_roles(bundle: &WorkflowBundle, subagents: &SubagentTool) -> Vec<&'static str> {
    let mut roles = Vec::new();
    for wf in std::iter::once(&bundle.entry).chain(bundle.workflows.values()) {
        collect_roles(&wf.body, &mut roles);
    }
    let mut missing: Vec<&'static str> = roles
        .into_iter()
        .filter(|role| !subagents.role_configured(*role))
        .map(ModelRole::as_str)
        .collect();
    missing.sort_unstable();
    missing.dedup();
    missing
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

    #[must_use]
    pub fn bind(
        &self,
        progress: ToolStreamPort,
        cancellation: CancellationSignal,
        monitor: Option<rho_harness_core::agents::AgentMonitor>,
        project: Option<std::sync::Arc<crate::engine::context::ProjectContext>>,
        session: Option<rho_harness_core::session::SessionManager>,
    ) -> Self {
        let ctx = TurnToolContext {
            permission: None,
            lifecycle: None,
            cancellation: Some(cancellation),
            progress,
            parent_history: std::sync::Arc::from(Vec::new()),
            monitor,
            project,
            session,
        };
        Self {
            subagents: self.subagents.with_turn_context(&ctx),
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
        let missing = missing_roles(bundle, &self.subagents);
        if !missing.is_empty() {
            return failed(format!(
                "workflow '{name}' uses model roles that are not configured: {} (set models.<role> in config.toml)",
                missing.join(", ")
            ));
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
