use super::builtins::{git_commit, repo_scopes};
use super::report::{RunLog, StepStatus};
use super::shell::run_shell;
use crate::tools::engine_tool::TurnToolContext;
use crate::tools::subagent::SubagentTool;
use futures::StreamExt;
use futures::future::BoxFuture;
use rho_harness_core::workflow::template::{is_truthy, lookup, render, value_to_text};
use rho_harness_core::workflow::{
    Builtin, OutputFormat, Step, StepKind, Workflow, WorkflowBundle, WorkflowCall, WorkflowCatalog,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

pub(crate) const CANCELLED: &str = "cancelled by operator";

pub(crate) struct Executor<'a> {
    pub subagents: &'a SubagentTool,
    pub catalog: &'a WorkflowCatalog,
    pub base_dir: &'a Path,
    pub log: &'a RunLog,
}

pub(crate) struct Frame<'a> {
    pub bundle: &'a WorkflowBundle,
    pub workflow: &'a Workflow,
    pub path: String,
    pub scope: Map<String, Value>,
}

pub(crate) struct StepOutcome {
    pub value: Value,
    pub status: StepStatus,
    pub detail: String,
}

impl StepOutcome {
    pub(crate) fn done(value: Value, detail: impl Into<String>) -> Self {
        Self {
            value,
            status: StepStatus::Done,
            detail: detail.into(),
        }
    }
}

pub(crate) fn frame_scope(inputs: Map<String, Value>, round: usize) -> Map<String, Value> {
    let mut scope = Map::new();
    scope.insert("inputs".to_string(), Value::Object(inputs));
    scope.insert("round".to_string(), json!(round));
    scope
}

/// A template that is exactly one placeholder yields the referenced value itself, so arrays and objects pass through.
pub(crate) fn render_value(template: &str, scope: &Value) -> Result<Value, String> {
    let trimmed = template.trim();
    let single = trimmed
        .strip_prefix("{{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .filter(|inner| !inner.contains("{{") && !inner.contains("}}"));
    match single {
        Some(path) => lookup(scope, path.trim())
            .cloned()
            .ok_or_else(|| format!("'{trimmed}' has no value")),
        None => render(template, scope).map(Value::String),
    }
}

fn render_map(map: &BTreeMap<String, String>, scope: &Value) -> Result<BTreeMap<String, String>, String> {
    map.iter()
        .map(|(k, v)| render(v, scope).map(|text| (k.clone(), text)))
        .collect()
}

pub(crate) fn collect_outputs(outputs: &BTreeMap<String, Vec<String>>, scope: &Value) -> Value {
    let mut collected = Map::new();
    for (name, refs) in outputs {
        let value = match refs.as_slice() {
            [single] => lookup(scope, single).cloned().unwrap_or(Value::Null),
            many => Value::Array(many.iter().flat_map(|r| flatten(lookup(scope, r))).collect()),
        };
        collected.insert(name.clone(), value);
    }
    Value::Object(collected)
}

fn flatten(value: Option<&Value>) -> Vec<Value> {
    match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(other) => vec![other.clone()],
    }
}

pub(crate) fn fingerprints(value: &Value) -> BTreeSet<String> {
    flatten(Some(value))
        .iter()
        .map(|item| match item.get("fingerprint").and_then(Value::as_str) {
            Some(fp) => fp.to_string(),
            None => item.to_string(),
        })
        .collect()
}

pub(crate) fn summarize(value: &Value) -> String {
    match value {
        Value::Object(map) => map
            .iter()
            .take(4)
            .map(|(k, v)| match v {
                Value::Array(items) => format!("{k}: {}", items.len()),
                Value::Object(_) => format!("{k}: {{…}}"),
                other => format!("{k}: {}", value_to_text(other)),
            })
            .collect::<Vec<_>>()
            .join(", "),
        Value::Array(items) => format!("{} items", items.len()),
        Value::String(text) => format!("{} lines", text.lines().count()),
        other => value_to_text(other),
    }
}

impl Executor<'_> {
    pub(crate) fn ctx(&self) -> &TurnToolContext {
        self.subagents.turn_context()
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.ctx().cancellation.as_ref().is_some_and(|c| c.is_cancelled())
    }

    pub(crate) fn run_workflow<'f>(&'f self, frame: Frame<'f>) -> BoxFuture<'f, Result<Value, String>> {
        Box::pin(self.run_workflow_inner(frame))
    }

    async fn run_workflow_inner(&self, mut frame: Frame<'_>) -> Result<Value, String> {
        for step in &frame.workflow.steps {
            if self.cancelled() {
                return Err(CANCELLED.to_string());
            }
            let step_path = format!("{} › {}", frame.path, step.id);
            let scope = Value::Object(frame.scope.clone());
            if let Some(when) = &step.when
                && !lookup(&scope, when).is_some_and(is_truthy)
            {
                self.log
                    .record(&step_path, StepStatus::Skipped, format!("`{when}` is empty"));
                frame.scope.insert(step.id.clone(), Value::Null);
                continue;
            }
            self.ctx().progress.stream_chunk(&format!("{step_path}\n"));
            let outcome = match self.run_step(&frame, step, &step_path, &scope).await {
                Ok(outcome) => outcome,
                Err(error) => return Err(self.fail_step(step, &step_path, error)),
            };
            self.log.record(&step_path, outcome.status, outcome.detail);
            frame.scope.insert(step.id.clone(), outcome.value);
        }
        Ok(collect_outputs(&frame.workflow.outputs, &Value::Object(frame.scope)))
    }

    fn fail_step(&self, step: &Step, step_path: &str, error: String) -> String {
        if matches!(step.kind, StepKind::Workflow(_)) {
            return error;
        }
        self.log.record(step_path, StepStatus::Failed, error.clone());
        format!("{step_path}: {error}")
    }

    async fn run_step(&self, frame: &Frame<'_>, step: &Step, path: &str, scope: &Value) -> Result<StepOutcome, String> {
        let Some(over) = &step.over else {
            return self.run_single(frame, step, path, scope, None).await;
        };
        let items = lookup(scope, over)
            .and_then(Value::as_array)
            .ok_or_else(|| format!("`over = \"{over}\"` must reference a list"))?;
        let item_scopes: Vec<Value> = items
            .iter()
            .map(|item| {
                let mut item_scope = scope.clone();
                if let Some(map) = item_scope.as_object_mut() {
                    map.insert("item".to_string(), item.clone());
                }
                item_scope
            })
            .collect();
        let mut runs = Vec::with_capacity(item_scopes.len());
        for (index, item_scope) in item_scopes.iter().enumerate() {
            runs.push(self.run_single(frame, step, path, item_scope, Some(index + 1)));
        }
        let results: Vec<Result<StepOutcome, String>> =
            futures::stream::iter(runs).buffered(step.concurrency).collect().await;
        if self.cancelled() {
            return Err(CANCELLED.to_string());
        }
        let failed = results.iter().filter(|r| r.is_err()).count();
        let values: Vec<Value> = results
            .into_iter()
            .map(|r| r.map_or_else(|e| Value::String(format!("[item failed: {e}]")), |o| o.value))
            .collect();
        let detail = match failed {
            0 => format!("{} items", values.len()),
            n => format!("{} items, {n} failed", values.len()),
        };
        Ok(StepOutcome::done(Value::Array(values), detail))
    }

    async fn run_single(
        &self,
        frame: &Frame<'_>,
        step: &Step,
        path: &str,
        scope: &Value,
        index: Option<usize>,
    ) -> Result<StepOutcome, String> {
        match &step.kind {
            StepKind::Agent(agent) => self.run_agent_step(frame, step, agent, scope, index).await,
            StepKind::Run { command } => self.run_command(step, command, scope).await,
            StepKind::Builtin { builtin, with } => self.run_builtin(*builtin, with, scope).await,
            StepKind::Workflow(call) => self.run_call(frame, call, path, scope).await,
        }
    }

    async fn run_command(&self, step: &Step, command: &str, scope: &Value) -> Result<StepOutcome, String> {
        let command = render(command, scope)?;
        let timeout = Duration::from_secs(step.timeout_sec);
        let outcome = run_shell(self.ctx(), self.base_dir, &command, timeout).await?;
        if !outcome.success() {
            return Err(outcome.failure_summary(&command));
        }
        let value = match step.output {
            OutputFormat::Json => serde_json::from_str(outcome.stdout.trim())
                .map_err(|e| format!("`{command}` did not print valid JSON: {e}"))?,
            OutputFormat::Text => Value::String(outcome.stdout.trim_end().to_string()),
        };
        Ok(StepOutcome::done(value, format!("`{command}` succeeded")))
    }

    async fn run_builtin(
        &self,
        builtin: Builtin,
        with: &BTreeMap<String, String>,
        scope: &Value,
    ) -> Result<StepOutcome, String> {
        let with = render_map(with, scope)?;
        match builtin {
            Builtin::RepoScopes => {
                let value = repo_scopes(self.base_dir, &with).await?;
                let count = value["items"].as_array().map_or(0, Vec::len);
                let files = value["reviewable_files"].as_u64().unwrap_or(0);
                Ok(StepOutcome::done(value, format!("{count} scopes over {files} files")))
            }
            Builtin::GitCommit => {
                let message = with
                    .get("message")
                    .cloned()
                    .unwrap_or_else(|| "chore: workflow checkpoint".to_string());
                let value = git_commit(self.ctx(), self.base_dir, &message).await?;
                let detail = value["sha"]
                    .as_str()
                    .map_or_else(|| "nothing to commit".to_string(), |sha| format!("committed {sha}"));
                Ok(StepOutcome::done(value, detail))
            }
        }
    }

    fn resolve_call<'b>(
        &'b self,
        bundle: &'b WorkflowBundle,
        name: &str,
    ) -> Result<(&'b WorkflowBundle, &'b Workflow), String> {
        if let Some(local) = bundle.workflows.get(name) {
            return Ok((bundle, local));
        }
        self.catalog
            .find(name)
            .map(|other| (other, &other.entry))
            .ok_or_else(|| format!("unknown workflow '{name}'"))
    }

    async fn run_call(
        &self,
        frame: &Frame<'_>,
        call: &WorkflowCall,
        path: &str,
        scope: &Value,
    ) -> Result<StepOutcome, String> {
        let (bundle, target) = self.resolve_call(frame.bundle, &call.name)?;
        let mut provided = Map::new();
        for (key, template) in &call.with {
            provided.insert(key.clone(), render_value(template, scope)?);
        }
        if call.repeat.is_none() {
            let inputs = target.resolve_inputs(provided)?;
            let child = Frame {
                bundle,
                workflow: target,
                path: path.to_string(),
                scope: frame_scope(inputs, 1),
            };
            let outputs = self.run_workflow(child).await?;
            return Ok(StepOutcome::done(outputs, ""));
        }
        self.run_repeat(RepeatPlan {
            bundle,
            target,
            call,
            path,
            provided,
            scope,
        })
        .await
    }
}

pub(crate) struct RepeatPlan<'a> {
    pub bundle: &'a WorkflowBundle,
    pub target: &'a Workflow,
    pub call: &'a WorkflowCall,
    pub path: &'a str,
    pub provided: Map<String, Value>,
    pub scope: &'a Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_value_passes_single_references_through_unchanged() {
        let scope = json!({"inputs": {"excluded": [1, 2], "profile": "security"}, "round": 3});
        assert_eq!(render_value("{{ inputs.excluded }}", &scope).unwrap(), json!([1, 2]));
        assert_eq!(render_value("{{round}}", &scope).unwrap(), json!(3));
        assert_eq!(
            render_value("fix({{inputs.profile}}) r{{round}}", &scope).unwrap(),
            json!("fix(security) r3")
        );
        assert!(render_value("{{inputs.nope}}", &scope).is_err());
    }

    #[test]
    fn outputs_take_single_values_or_concatenate_lists() {
        let scope = json!({
            "inputs": {"excluded": [{"fingerprint": "a"}]},
            "implement": {"wont_fix": [{"fingerprint": "b"}]},
            "validate": {"actionable": []},
        });
        let mut outputs = BTreeMap::new();
        outputs.insert("actionable".to_string(), vec!["validate.actionable".to_string()]);
        outputs.insert(
            "excluded".to_string(),
            vec![
                "inputs.excluded".to_string(),
                "implement.wont_fix".to_string(),
                "skipped.x".to_string(),
            ],
        );
        outputs.insert("missing".to_string(), vec!["ghost".to_string()]);
        assert_eq!(
            collect_outputs(&outputs, &scope),
            json!({"actionable": [], "excluded": [{"fingerprint": "a"}, {"fingerprint": "b"}], "missing": null})
        );
    }

    #[test]
    fn fingerprints_prefer_the_fingerprint_field() {
        let value = json!([{"fingerprint": "src/a.rs:x", "line": 3}, {"title": "no fp"}]);
        let fps = fingerprints(&value);
        assert!(fps.contains("src/a.rs:x"));
        assert!(fps.contains("{\"title\":\"no fp\"}"));
    }

    #[test]
    fn summaries_describe_shapes_briefly() {
        assert_eq!(
            summarize(&json!({"actionable": [1, 2], "rejected": 3})),
            "actionable: 2, rejected: 3"
        );
        assert_eq!(summarize(&json!([1, 2, 3])), "3 items");
        assert_eq!(summarize(&json!("a\nb")), "2 lines");
    }
}
