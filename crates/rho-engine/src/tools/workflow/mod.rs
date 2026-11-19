mod agent_step;
mod command;
mod executor;
mod report;
#[cfg(test)]
mod tests;

use crate::engine::runner::turn::types::CancellationSignal;
use crate::tools::engine_tool::TurnToolContext;
use crate::tools::subagent::SubagentTool;
use executor::{Executor, Frame};
use report::RunLog;
use rho_harness_core::presentation::ToolStreamPort;
use rho_harness_core::workflow::{ModelRole, Node, NodeKind, Workflow, WorkflowBundle, WorkflowCatalog};
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
        let branch = match self.isolate(&bundle.entry).await {
            Ok(branch) => branch,
            Err(e) => {
                let report = log.report(&format!("Workflow `{}` did not start: {e}", bundle.name), None);
                return failed(report);
            }
        };
        let executor = Executor {
            subagents: &self.subagents,
            catalog,
            base_dir: &self.base_dir,
            log: &log,
        };
        let frame = Frame {
            bundle,
            path: bundle.name.clone(),
        };
        let result = executor.run_workflow(frame, &bundle.entry, inputs).await;
        log.event("run_finished", json!({ "ok": result.is_ok() }));
        let on_branch = branch.map(|b| format!(" on branch `{b}`")).unwrap_or_default();
        match result {
            Ok(outputs) => WorkflowRun {
                ok: true,
                report: log.report(
                    &format!("Workflow `{}` completed{on_branch}.", bundle.name),
                    Some(&outputs),
                ),
                started: true,
            },
            Err(e) => WorkflowRun {
                ok: false,
                report: log.report(&format!("Workflow `{}` stopped{on_branch}: {e}", bundle.name), None),
                started: true,
            },
        }
    }

    async fn isolate(&self, workflow: &Workflow) -> Result<Option<String>, String> {
        match workflow.isolation {
            rho_harness_core::workflow::Isolation::None => Ok(None),
            rho_harness_core::workflow::Isolation::Branch => {
                command::start_branch(&self.base_dir, &workflow.name).await.map(Some)
            }
        }
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
