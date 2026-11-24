use super::command::{CANCELLED, run_command};
use super::report::{RunLog, StepStatus};
use super::resume::Replay;
use super::write_step::write_output;
use crate::tools::engine_tool::TurnToolContext;
use crate::tools::subagent::SubagentTool;
use futures::StreamExt;
use futures::future::BoxFuture;
use rho_harness_core::workflow::parse::RUN_SCOPE;
use rho_harness_core::workflow::template::{evaluate_with, is_truthy, lookup, render_with};
use rho_harness_core::workflow::{
    CallNode, ChooseNode, EachNode, Items, LoopNode, Node, NodeKind, OutputFormat, RunNode, Workflow, WorkflowBundle,
    WorkflowCatalog, WriteNode,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Default)]
pub(crate) struct Scope {
    values: Map<String, Value>,
    history: BTreeMap<String, Vec<Value>>,
}

impl Scope {
    pub(crate) fn new(inputs: &BTreeMap<String, String>, run_id: &str) -> Self {
        let mut values: Map<String, Value> = inputs
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        values.insert(RUN_SCOPE.to_string(), json!({ "id": run_id }));
        Self {
            values,
            history: BTreeMap::new(),
        }
    }

    pub(crate) fn set(&mut self, id: &str, value: Value) {
        if !value.is_null() {
            self.history.entry(id.to_string()).or_default().push(value.clone());
        }
        self.values.insert(id.to_string(), value);
    }

    fn clear(&mut self, id: &str) {
        self.values.insert(id.to_string(), Value::Null);
    }

    fn bind(&mut self, name: &str, value: Value) {
        self.values.insert(name.to_string(), value);
    }

    fn resolve(&self, path: &str) -> Option<Value> {
        if let Some(id) = path.strip_suffix(".history").filter(|id| !id.contains('.')) {
            return Some(Value::Array(self.history.get(id).cloned().unwrap_or_default()));
        }
        let (root, rest) = path.split_once('.').map_or((path, None), |(r, rest)| (r, Some(rest)));
        let value = self.values.get(root)?;
        match rest {
            Some(rest) => lookup(value, rest).cloned(),
            None => Some(value.clone()),
        }
    }

    pub(crate) fn render(&self, template: &str) -> String {
        render_with(template, &|path| self.resolve(path))
    }

    pub(crate) fn evaluate(&self, template: &str) -> Value {
        evaluate_with(template, &|path| self.resolve(path))
    }

    pub(crate) fn get(&self, id: &str) -> Value {
        self.values.get(id).cloned().unwrap_or(Value::Null)
    }
}

pub(crate) struct Executor<'a> {
    pub subagents: &'a SubagentTool,
    pub catalog: &'a WorkflowCatalog,
    pub base_dir: &'a Path,
    pub project_dir: &'a Path,
    pub replay: Option<&'a Replay>,
    pub log: &'a RunLog,
}

#[derive(Clone)]
pub(crate) struct Frame<'a> {
    pub bundle: &'a WorkflowBundle,
    pub path: String,
}

impl Frame<'_> {
    pub(crate) fn child(&self, label: &str) -> Self {
        Self {
            bundle: self.bundle,
            path: format!("{} › {label}", self.path),
        }
    }
}

pub(crate) fn node_label(node: &Node) -> String {
    match (&node.id, &node.kind) {
        (Some(id), _) => id.clone(),
        (None, NodeKind::Agent(agent)) if !agent.agent.contains("{{") => agent.agent.clone(),
        (None, NodeKind::Agent(_)) => "agent".to_string(),
        (None, kind) => kind.element_name().to_string(),
    }
}

pub(crate) fn collect_ids(nodes: &[Node], into: &mut BTreeSet<String>) {
    for node in nodes {
        if let Some(id) = &node.id {
            into.insert(id.clone());
        }
        for body in node.kind.bodies() {
            collect_ids(body, into);
        }
    }
}

fn parse_max(value: &Value) -> Result<usize, String> {
    let parsed = match value {
        Value::Number(n) => n.as_u64().and_then(|n| usize::try_from(n).ok()),
        Value::String(s) => s.trim().parse::<usize>().ok(),
        _ => None,
    };
    parsed
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("loop max must be a positive integer (got {value})"))
}

fn json_from_stdout(stdout: &str) -> Result<Value, String> {
    serde_json::from_str(stdout.trim()).map_err(|e| format!("command did not print valid JSON: {e}"))
}

impl Executor<'_> {
    pub(crate) fn ctx(&self) -> &TurnToolContext {
        self.subagents.turn_context()
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.ctx().cancellation.as_ref().is_some_and(|c| c.is_cancelled())
    }

    pub(crate) async fn run_workflow(
        &self,
        frame: Frame<'_>,
        workflow: &Workflow,
        inputs: &BTreeMap<String, String>,
    ) -> Result<Value, String> {
        let mut scope = Scope::new(inputs, self.log.id());
        self.run_nodes(frame, &workflow.body, &mut scope).await?;
        let outputs: Map<String, Value> = workflow
            .outputs
            .iter()
            .map(|(name, template)| (name.clone(), scope.evaluate(template)))
            .collect();
        Ok(Value::Object(outputs))
    }

    fn run_nodes<'f>(
        &'f self,
        frame: Frame<'f>,
        nodes: &'f [Node],
        scope: &'f mut Scope,
    ) -> BoxFuture<'f, Result<Value, String>> {
        Box::pin(async move {
            let mut last = Value::Null;
            for node in nodes {
                if self.cancelled() {
                    return Err(CANCELLED.to_string());
                }
                last = self.run_node(&frame, node, scope).await?;
            }
            Ok(last)
        })
    }

    fn should_run(node: &Node, scope: &Scope) -> bool {
        let when = node.when.as_ref().is_none_or(|t| is_truthy(&scope.evaluate(t)));
        let unless = node.unless.as_ref().is_some_and(|t| is_truthy(&scope.evaluate(t)));
        when && !unless
    }

    async fn run_node(&self, frame: &Frame<'_>, node: &Node, scope: &mut Scope) -> Result<Value, String> {
        let label = node_label(node);
        let path = frame.child(&label).path;
        if !Self::should_run(node, scope) {
            self.log.record(&path, StepStatus::Skipped, "condition not met");
            if let Some(id) = &node.id {
                scope.clear(id);
            }
            return Ok(Value::Null);
        }
        let value = match &node.kind {
            NodeKind::Agent(_) | NodeKind::Run(_) | NodeKind::Write(_) | NodeKind::Call(_) => {
                self.run_leaf(frame, node, &label, scope).await?
            }
            NodeKind::Sequence(body) => self.run_nodes(frame.clone(), body, scope).await?,
            NodeKind::Parallel(children) => self.run_parallel(frame, children, scope).await?,
            NodeKind::Each(each) => self.run_each(frame, &label, each, scope).await?,
            NodeKind::Loop(l) => self.run_loop(frame, node, &label, l, scope).await?,
            NodeKind::Choose(c) => self.run_choose(frame, c, scope).await?,
        };
        if let Some(id) = &node.id {
            scope.set(id, value.clone());
        }
        Ok(value)
    }

    async fn run_leaf(&self, frame: &Frame<'_>, node: &Node, label: &str, scope: &Scope) -> Result<Value, String> {
        let path = frame.child(label).path;
        self.ctx().progress.stream_chunk(&format!("{path}\n"));
        let result = match &node.kind {
            NodeKind::Agent(agent) => self.run_agent(frame, label, agent, scope).await,
            NodeKind::Run(run) => self.run_command_step(&path, run, scope).await,
            NodeKind::Write(write) => self.run_write_step(write, scope).await,
            NodeKind::Call(call) => self.run_call(frame, label, call, scope).await,
            _ => unreachable!("run_leaf only receives agent, run, write, and call nodes"),
        };
        match result {
            Ok((value, detail)) => {
                self.log.record(&path, StepStatus::Done, detail);
                Ok(value)
            }
            Err(e) if e == CANCELLED => Err(e),
            Err(e) => {
                self.log.record(&path, StepStatus::Failed, e.clone());
                Err(format!("{path}: {e}"))
            }
        }
    }

    async fn run_command_step(&self, path: &str, run: &RunNode, scope: &Scope) -> Result<(Value, String), String> {
        let args: Vec<String> = run.args.iter().map(|a| scope.render(a)).collect();
        self.log.event("command", json!({ "step": path, "args": args }));
        let outcome = run_command(&args, self.base_dir, self.ctx().cancellation.as_ref()).await?;
        let json = match run.output {
            OutputFormat::Json if outcome.ok() => Some(json_from_stdout(&outcome.stdout)?),
            _ => None,
        };
        let detail = match outcome.exit {
            Some(0) => "ok".to_string(),
            Some(code) => format!("exit {code}"),
            None => "terminated by a signal".to_string(),
        };
        Ok((outcome.to_value(json), detail))
    }

    async fn run_write_step(&self, write: &WriteNode, scope: &Scope) -> Result<(Value, String), String> {
        let path = scope.render(&write.path);
        let detail = write_output(self.project_dir, &path, scope.render(&write.content)).await?;
        Ok((Value::String(path.trim().to_string()), detail))
    }

    async fn run_call(
        &self,
        frame: &Frame<'_>,
        label: &str,
        call: &CallNode,
        scope: &Scope,
    ) -> Result<(Value, String), String> {
        let (bundle, target) = self
            .catalog
            .resolve_call(frame.bundle, &call.workflow)
            .ok_or_else(|| format!("unknown workflow '{}'", call.workflow))?;
        let provided = call.with.iter().map(|(k, t)| (k.clone(), scope.render(t))).collect();
        let inputs = target.resolve_inputs(provided)?;
        let child = Frame {
            bundle,
            path: frame.child(label).path,
        };
        let outputs = Box::pin(self.run_workflow(child, target, &inputs)).await?;
        Ok((outputs, String::new()))
    }

    async fn run_parallel(&self, frame: &Frame<'_>, children: &[Node], scope: &mut Scope) -> Result<Value, String> {
        let mut branches: Vec<Scope> = children.iter().map(|_| scope.clone()).collect();
        let mut runs = Vec::with_capacity(children.len());
        for (child, branch) in children.iter().zip(branches.iter_mut()) {
            runs.push(self.run_nodes(frame.clone(), std::slice::from_ref(child), branch));
        }
        let results = futures::future::join_all(runs).await;
        for result in results {
            result?;
        }
        let mut value = Map::new();
        for (child, branch) in children.iter().zip(&branches) {
            let mut ids = BTreeSet::new();
            collect_ids(std::slice::from_ref(child), &mut ids);
            for id in ids {
                scope.set(&id, branch.get(&id));
            }
            if let Some(id) = &child.id {
                value.insert(id.clone(), branch.get(id));
            }
        }
        Ok(Value::Object(value))
    }

    fn each_items(each: &EachNode, scope: &Scope) -> Result<Vec<Value>, String> {
        match &each.items {
            Items::Literal(items) => Ok(items.iter().cloned().map(Value::String).collect()),
            Items::Reference(template) => match scope.evaluate(template) {
                Value::Array(items) => Ok(items),
                Value::Null => Ok(Vec::new()),
                other => Err(format!("each items must be a list (got {other})")),
            },
        }
    }

    async fn run_each(&self, frame: &Frame<'_>, label: &str, each: &EachNode, scope: &Scope) -> Result<Value, String> {
        let items = Self::each_items(each, scope)?;
        let mut item_scopes: Vec<(Frame<'_>, Scope)> = Vec::with_capacity(items.len());
        for (index, item) in items.into_iter().enumerate() {
            let mut item_scope = scope.clone();
            item_scope.bind(&each.var, item);
            item_scopes.push((frame.child(&format!("{label} #{}", index + 1)), item_scope));
        }
        let mut runs = Vec::with_capacity(item_scopes.len());
        for (item_frame, item_scope) in &mut item_scopes {
            runs.push(self.run_nodes(item_frame.clone(), &each.body, item_scope));
        }
        let results: Vec<Result<Value, String>> = futures::stream::iter(runs).buffered(each.parallel).collect().await;
        let mut values = Vec::with_capacity(results.len());
        for result in results {
            match result {
                Ok(value) => values.push(value),
                Err(e) if e == CANCELLED => return Err(e),
                Err(_) => values.push(Value::Null),
            }
        }
        Ok(Value::Array(values))
    }

    async fn run_loop(
        &self,
        frame: &Frame<'_>,
        node: &Node,
        label: &str,
        l: &LoopNode,
        scope: &mut Scope,
    ) -> Result<Value, String> {
        let max = parse_max(&scope.evaluate(&l.max))?;
        let mut ids = BTreeSet::new();
        collect_ids(&l.body, &mut ids);
        for id in &ids {
            scope.clear(id);
        }
        let (mut passes, mut stopped, mut previous) = (0, "max", None);
        for pass in 1..=max {
            if let Some(id) = &node.id {
                scope.bind(id, json!({ "index": pass }));
            }
            self.run_nodes(frame.child(&format!("{label} {pass}")), &l.body, scope)
                .await?;
            passes = pass;
            if let Some(reason) = Self::loop_stop(l, scope, &mut previous) {
                stopped = reason;
                break;
            }
        }
        let path = frame.child(label).path;
        self.log.record(
            &path,
            StepStatus::Done,
            format!("{passes} pass(es), stopped: {stopped}"),
        );
        Ok(json!({ "index": passes, "passes": passes, "stopped": stopped }))
    }

    fn loop_stop(l: &LoopNode, scope: &Scope, previous: &mut Option<Value>) -> Option<&'static str> {
        if l.until.as_ref().is_some_and(|t| is_truthy(&scope.evaluate(t))) {
            return Some("until");
        }
        if l.while_.as_ref().is_some_and(|t| !is_truthy(&scope.evaluate(t))) {
            return Some("while");
        }
        let current = l.stop_if_unchanged.as_ref().map(|t| scope.evaluate(t))?;
        if previous.as_ref() == Some(&current) {
            return Some("unchanged");
        }
        *previous = Some(current);
        None
    }

    async fn run_choose(&self, frame: &Frame<'_>, choose: &ChooseNode, scope: &mut Scope) -> Result<Value, String> {
        let on = scope.render(&choose.on);
        let on = on.trim();
        let branch = choose
            .cases
            .iter()
            .find(|(value, _)| value == on)
            .map(|(value, body)| (Value::String(value.clone()), body))
            .or_else(|| {
                choose
                    .otherwise
                    .as_ref()
                    .map(|body| (Value::String("otherwise".into()), body))
            });
        let Some((matched, body)) = branch else {
            return Ok(Value::Null);
        };
        self.run_nodes(frame.clone(), body, scope).await?;
        Ok(matched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Scope {
        let mut inputs = BTreeMap::new();
        inputs.insert("profile".to_string(), "security".to_string());
        Scope::new(&inputs, "test-run")
    }

    #[test]
    fn scope_resolves_inputs_fields_and_history() {
        let mut s = scope();
        s.set("verify", json!({ "ok": false, "output": "boom" }));
        s.set("verify", Value::Null);
        s.set("verify", json!({ "ok": true, "output": "fine" }));
        assert_eq!(s.render("{{profile}}: {{verify.output}}"), "security: fine");
        assert_eq!(
            s.evaluate("{{verify.history}}"),
            json!([{ "ok": false, "output": "boom" }, { "ok": true, "output": "fine" }])
        );
        assert_eq!(s.evaluate("{{other.history}}"), json!([]));
        assert_eq!(s.render("[{{missing.field}}]"), "[]");
    }

    #[test]
    fn loop_max_accepts_numbers_and_numeric_strings() {
        assert_eq!(parse_max(&json!(4)).unwrap(), 4);
        assert_eq!(parse_max(&json!("3")).unwrap(), 3);
        for bad in [json!(0), json!("x"), json!(null)] {
            assert!(parse_max(&bad).is_err(), "{bad}");
        }
    }
}
