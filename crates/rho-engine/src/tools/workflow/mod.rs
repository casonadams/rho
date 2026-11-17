mod agent_step;
mod builtins;
mod executor;
mod repeat;
mod report;
mod shell;
#[cfg(test)]
mod tests;

use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use crate::tools::subagent::SubagentTool;
use crate::tools::types::{ToolResult, generated_schema};
use executor::{Executor, Frame, frame_scope};
use report::RunLog;
use rho_harness_core::args::WorkflowArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::workflow::{Isolation, WorkflowBundle, WorkflowCatalog};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const WORKFLOW_TOOL_NAME: &str = "workflow";

#[derive(Clone)]
pub struct WorkflowTool {
    subagents: SubagentTool,
    base_dir: PathBuf,
    home_dir: Option<PathBuf>,
    description: String,
}

impl WorkflowTool {
    #[must_use]
    pub fn new(subagents: SubagentTool, base_dir: &Path, home_dir: Option<PathBuf>) -> Self {
        let catalog = WorkflowCatalog::discover(Some(base_dir), home_dir.as_deref());
        let available = catalog
            .bundles
            .iter()
            .map(|b| format!("{} {}", b.name, b.entry.argument_hint()).trim().to_string())
            .collect::<Vec<_>>()
            .join("; ");
        Self {
            subagents,
            base_dir: base_dir.to_path_buf(),
            home_dir,
            description: format!(
                "Run a declarative multi-agent workflow to completion and return its report. Run one only when the user asks for it. Available: {available}."
            ),
        }
    }

    fn catalog(&self) -> WorkflowCatalog {
        WorkflowCatalog::discover(Some(&self.base_dir), self.home_dir.as_deref())
    }

    async fn run(&self, catalog: &WorkflowCatalog, bundle: &WorkflowBundle, inputs: Map<String, Value>) -> ToolResult {
        let log = match RunLog::create(&self.base_dir, &bundle.name) {
            Ok(log) => log,
            Err(e) => return ToolResult::error(format!("workflow '{}' could not start: {e}", bundle.name)),
        };
        log.event(
            "run_started",
            json!({ "workflow": bundle.name, "location": bundle.location, "inputs": inputs }),
        );
        let ctx = self.subagents.turn_context();
        ctx.progress.stream_chunk(&format!(
            "workflow {} · press ← on an empty prompt to watch its agents\n",
            bundle.name
        ));
        let branch = match self.isolate(ctx, bundle).await {
            Ok(branch) => branch,
            Err(e) => {
                return ToolResult::error(log.report(&format!("Workflow `{}` did not start: {e}", bundle.name), None));
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
            workflow: &bundle.entry,
            path: bundle.name.clone(),
            scope: frame_scope(inputs, 1),
        };
        let result = executor.run_workflow(frame).await;
        let on_branch = branch.map(|b| format!(" on branch `{b}`")).unwrap_or_default();
        log.event("run_finished", json!({ "ok": result.is_ok() }));
        match result {
            Ok(outputs) => ToolResult::success(log.report(
                &format!("Workflow `{}` completed{on_branch}.", bundle.name),
                Some(&outputs),
            )),
            Err(e) => {
                ToolResult::error(log.report(&format!("Workflow `{}` stopped{on_branch}: {e}", bundle.name), None))
            }
        }
    }

    async fn isolate(&self, ctx: &TurnToolContext, bundle: &WorkflowBundle) -> Result<Option<String>, String> {
        match bundle.entry.isolation {
            Isolation::None => Ok(None),
            Isolation::Branch => builtins::start_branch(ctx, &self.base_dir, &bundle.name)
                .await
                .map(Some),
        }
    }
}

fn unknown_workflow(name: &str, catalog: &WorkflowCatalog) -> String {
    let names: Vec<&str> = catalog.bundles.iter().map(|b| b.name.as_str()).collect();
    let mut message = format!("unknown workflow '{name}'; available: {}", names.join(", "));
    if let Some(problem) = catalog.errors.iter().find(|e| e.contains(&format!("'{name}'"))) {
        message.push_str(&format!("\n{problem}"));
    }
    message
}

#[async_trait::async_trait]
impl EngineTool for WorkflowTool {
    fn name(&self) -> &str {
        WORKFLOW_TOOL_NAME
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        generated_schema::<WorkflowArgs>()
    }

    async fn execute(&self, args: Value) -> Result<ToolResult, AppError> {
        let args: WorkflowArgs = match serde_json::from_value(args) {
            Ok(parsed) => parsed,
            Err(err) => return Ok(ToolResult::error(format!("failed to parse workflow arguments: {err}"))),
        };
        let tool = self.clone();
        let catalog = tokio::task::spawn_blocking(move || tool.catalog())
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("workflow discovery failed: {e}")))?;
        let Some(bundle) = catalog.find(&args.name) else {
            return Ok(ToolResult::error(unknown_workflow(&args.name, &catalog)));
        };
        let inputs = match bundle.entry.resolve_inputs(args.inputs.into_iter().collect()) {
            Ok(inputs) => inputs,
            Err(e) => return Ok(ToolResult::error(e)),
        };
        Ok(self.run(&catalog, bundle, inputs).await)
    }

    fn bind_turn(&self, ctx: &TurnToolContext) -> Option<Arc<dyn EngineTool>> {
        Some(Arc::new(Self {
            subagents: self.subagents.with_turn_context(ctx),
            ..self.clone()
        }))
    }
}
