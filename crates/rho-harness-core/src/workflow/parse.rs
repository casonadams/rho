use super::template::references;
use super::types::{
    AgentStep, Builtin, Check, DEFAULT_CONCURRENCY, DEFAULT_TIMEOUT_SEC, InputSpec, Isolation, OnFail, OutputFormat,
    RESERVED_SCOPE_NAMES, Repeat, Step, StepKind, Workflow, WorkflowCall,
};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkflow {
    name: Option<String>,
    description: Option<String>,
    isolation: Option<String>,
    #[serde(default)]
    input: Vec<RawInput>,
    #[serde(default)]
    step: Vec<RawStep>,
    #[serde(default)]
    outputs: BTreeMap<String, RawRefs>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInput {
    name: String,
    description: Option<String>,
    #[serde(default)]
    choices: Vec<String>,
    default: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawRefs {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawMax {
    Number(usize),
    Text(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRepeat {
    until_empty: String,
    max: RawMax,
    #[serde(default)]
    stop_on_no_progress: bool,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawStep {
    id: String,
    agent: Option<String>,
    prompt: Option<String>,
    prompt_file: Option<String>,
    run: Option<String>,
    builtin: Option<String>,
    workflow: Option<String>,
    #[serde(default)]
    with: BTreeMap<String, serde_json::Value>,
    over: Option<String>,
    concurrency: Option<usize>,
    #[serde(default)]
    context: Vec<String>,
    when: Option<String>,
    expected_output: Option<String>,
    output: Option<String>,
    check: Option<String>,
    retries: Option<usize>,
    on_fail: Option<String>,
    max_turns: Option<usize>,
    model: Option<String>,
    timeout_sec: Option<u64>,
    repeat: Option<RawRepeat>,
    #[serde(default)]
    carry: Vec<String>,
}

pub type FileReader<'a> = &'a dyn Fn(&str) -> Result<String, String>;

struct Validator<'a> {
    inputs: Vec<String>,
    seen: Vec<String>,
    errors: Vec<String>,
    read_file: FileReader<'a>,
}

impl Validator<'_> {
    fn error(&mut self, step: &str, message: impl std::fmt::Display) {
        self.errors.push(format!("step '{step}': {message}"));
    }

    fn check_path(&mut self, step: &str, path: &str, allow_item: bool) {
        let mut segments = path.split('.');
        let head = segments.next().unwrap_or_default();
        let ok = match head {
            "inputs" => segments
                .next()
                .is_some_and(|name| self.inputs.iter().any(|i| i == name)),
            "item" => allow_item,
            "round" => true,
            other => self.seen.iter().any(|s| s == other),
        };
        if !ok {
            let hint = if head == "item" {
                " (only valid in steps with `over`)"
            } else {
                ""
            };
            self.error(step, format!("unknown reference '{path}'{hint}"));
        }
    }

    fn check_template(&mut self, step: &str, template: &str, allow_item: bool) {
        match references(template) {
            Ok(paths) => paths.iter().for_each(|p| self.check_path(step, p, allow_item)),
            Err(e) => self.error(step, e),
        }
    }
}

pub fn parse_workflow(default_name: &str, text: &str, read_file: FileReader<'_>) -> Result<Workflow, String> {
    let raw: RawWorkflow = toml::from_str(text).map_err(|e| format!("invalid workflow TOML: {e}"))?;
    let name = raw.name.clone().unwrap_or_else(|| default_name.to_string());
    let mut v = Validator {
        inputs: raw.input.iter().map(|i| i.name.clone()).collect(),
        seen: Vec::new(),
        errors: Vec::new(),
        read_file,
    };
    let isolation = parse_isolation(raw.isolation.as_deref(), &mut v);
    let steps: Vec<Step> = raw.step.into_iter().filter_map(|s| convert_step(s, &mut v)).collect();
    let outputs = convert_outputs(raw.outputs, &mut v);
    if steps.is_empty() && v.errors.is_empty() {
        v.errors.push("workflow has no steps".to_string());
    }
    if !v.errors.is_empty() {
        return Err(format!("workflow '{name}' is invalid:\n- {}", v.errors.join("\n- ")));
    }
    Ok(Workflow {
        name,
        description: raw.description.unwrap_or_default(),
        isolation,
        inputs: raw.input.into_iter().map(convert_input).collect(),
        steps,
        outputs,
    })
}

fn parse_isolation(value: Option<&str>, v: &mut Validator<'_>) -> Isolation {
    match value {
        None | Some("none") => Isolation::None,
        Some("branch") => Isolation::Branch,
        Some(other) => {
            v.errors
                .push(format!("isolation must be 'none' or 'branch' (got '{other}')"));
            Isolation::None
        }
    }
}

fn convert_input(raw: RawInput) -> InputSpec {
    InputSpec {
        name: raw.name,
        description: raw.description,
        choices: raw.choices,
        default: raw.default,
    }
}

fn convert_outputs(raw: BTreeMap<String, RawRefs>, v: &mut Validator<'_>) -> BTreeMap<String, Vec<String>> {
    raw.into_iter()
        .map(|(name, refs)| {
            let refs = match refs {
                RawRefs::One(r) => vec![r],
                RawRefs::Many(rs) => rs,
            };
            for r in &refs {
                v.check_path(&format!("outputs.{name}"), r, false);
            }
            (name, refs)
        })
        .collect()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !RESERVED_SCOPE_NAMES.contains(&id)
}

fn convert_step(raw: RawStep, v: &mut Validator<'_>) -> Option<Step> {
    let id = raw.id.clone();
    if !valid_id(&id) {
        v.errors.push(format!(
            "invalid step id '{id}' (use letters, digits, '-', '_'; not inputs/item/round)"
        ));
    } else if v.seen.contains(&id) {
        v.error(&id, "duplicate step id");
    }
    let allow_item = raw.over.is_some();
    check_common_refs(&raw, v, allow_item);
    let kind = convert_kind(&raw, v, allow_item);
    let check = convert_check(&raw, v, matches!(kind, Some(StepKind::Agent(_))));
    let output = convert_output(raw.output.as_deref(), &id, v);
    v.seen.push(id.clone());
    Some(Step {
        id,
        kind: kind?,
        over: raw.over,
        concurrency: raw.concurrency.unwrap_or(DEFAULT_CONCURRENCY).max(1),
        context: raw.context,
        when: raw.when,
        expected_output: raw.expected_output,
        output,
        check,
        timeout_sec: raw.timeout_sec.unwrap_or(DEFAULT_TIMEOUT_SEC).max(1),
    })
}

fn check_common_refs(raw: &RawStep, v: &mut Validator<'_>, allow_item: bool) {
    if let Some(over) = &raw.over {
        v.check_path(&raw.id, over, false);
    }
    if let Some(when) = &raw.when {
        v.check_path(&raw.id, when, false);
    }
    for ctx in &raw.context {
        if !v.seen.contains(ctx) {
            v.error(&raw.id, format!("context '{ctx}' must name an earlier step"));
        }
    }
    for value in raw.with.values() {
        if let Some(text) = value.as_str() {
            v.check_template(&raw.id, text, allow_item);
        }
    }
}

fn convert_output(value: Option<&str>, id: &str, v: &mut Validator<'_>) -> OutputFormat {
    match value {
        None | Some("text") => OutputFormat::Text,
        Some("json") => OutputFormat::Json,
        Some(other) => {
            v.error(id, format!("output must be 'text' or 'json' (got '{other}')"));
            OutputFormat::Text
        }
    }
}

fn convert_check(raw: &RawStep, v: &mut Validator<'_>, is_agent: bool) -> Option<Check> {
    let Some(command) = raw.check.clone() else {
        if raw.retries.is_some() || raw.on_fail.is_some() {
            v.error(&raw.id, "`retries` and `on_fail` require `check`");
        }
        return None;
    };
    if !is_agent {
        v.error(&raw.id, "`check` is only supported on agent steps");
    }
    v.check_template(&raw.id, &command, raw.over.is_some());
    let on_fail = match raw.on_fail.as_deref() {
        None | Some("stop") => OnFail::Stop,
        Some("stash") => OnFail::Stash,
        Some("continue") => OnFail::Continue,
        Some(other) => {
            v.error(
                &raw.id,
                format!("on_fail must be stop, stash, or continue (got '{other}')"),
            );
            OnFail::Stop
        }
    };
    Some(Check {
        command,
        retries: raw.retries.unwrap_or(0),
        on_fail,
    })
}

fn convert_kind(raw: &RawStep, v: &mut Validator<'_>, allow_item: bool) -> Option<StepKind> {
    let kinds = [
        raw.agent.is_some(),
        raw.run.is_some(),
        raw.builtin.is_some(),
        raw.workflow.is_some(),
    ];
    if kinds.iter().filter(|k| **k).count() != 1 {
        v.error(&raw.id, "set exactly one of `agent`, `run`, `builtin`, or `workflow`");
        return None;
    }
    if raw.agent.is_none() && (raw.prompt.is_some() || raw.prompt_file.is_some()) {
        v.error(&raw.id, "`prompt` and `prompt_file` are only valid on agent steps");
    }
    if raw.workflow.is_none() && (raw.repeat.is_some() || !raw.carry.is_empty()) {
        v.error(&raw.id, "`repeat` and `carry` are only valid on workflow steps");
    }
    if let Some(agent) = &raw.agent {
        return convert_agent(raw, agent, v, allow_item);
    }
    if let Some(command) = &raw.run {
        v.check_template(&raw.id, command, allow_item);
        return Some(StepKind::Run {
            command: command.clone(),
        });
    }
    if let Some(name) = &raw.builtin {
        return convert_builtin(raw, name, v);
    }
    raw.workflow.as_ref().and_then(|name| convert_call(raw, name, v))
}

fn convert_agent(raw: &RawStep, agent: &str, v: &mut Validator<'_>, allow_item: bool) -> Option<StepKind> {
    let prompt = match (&raw.prompt, &raw.prompt_file) {
        (Some(p), None) => p.clone(),
        (None, Some(path)) => match (v.read_file)(path) {
            Ok(text) => text,
            Err(e) => {
                v.error(&raw.id, format!("cannot read prompt_file '{path}': {e}"));
                return None;
            }
        },
        _ => {
            v.error(&raw.id, "agent steps need exactly one of `prompt` or `prompt_file`");
            return None;
        }
    };
    v.check_template(&raw.id, &prompt, allow_item);
    Some(StepKind::Agent(AgentStep {
        agent: agent.to_string(),
        prompt,
        max_turns: raw.max_turns,
        model: raw.model.clone(),
    }))
}

fn template_map(with: &BTreeMap<String, serde_json::Value>) -> BTreeMap<String, String> {
    with.iter()
        .map(|(k, value)| {
            let text = value.as_str().map_or_else(|| value.to_string(), str::to_string);
            (k.clone(), text)
        })
        .collect()
}

fn convert_builtin(raw: &RawStep, name: &str, v: &mut Validator<'_>) -> Option<StepKind> {
    let Some(builtin) = Builtin::parse(name) else {
        let known: Vec<&str> = Builtin::ALL.iter().map(|(n, _)| *n).collect();
        v.error(
            &raw.id,
            format!("unknown builtin '{name}' (known: {})", known.join(", ")),
        );
        return None;
    };
    Some(StepKind::Builtin {
        builtin,
        with: template_map(&raw.with),
    })
}

fn convert_call(raw: &RawStep, name: &str, v: &mut Validator<'_>) -> Option<StepKind> {
    let repeat = raw.repeat.as_ref().map(|r| {
        let max = match &r.max {
            RawMax::Number(n) => n.to_string(),
            RawMax::Text(t) => t.clone(),
        };
        check_repeat_max(&raw.id, &max, v);
        Repeat {
            until_empty: r.until_empty.clone(),
            max,
            stop_on_no_progress: r.stop_on_no_progress,
        }
    });
    if repeat.is_none() && !raw.carry.is_empty() {
        v.error(&raw.id, "`carry` requires `repeat`");
    }
    Some(StepKind::Workflow(WorkflowCall {
        name: name.to_string(),
        with: template_map(&raw.with),
        repeat,
        carry: raw.carry.clone(),
    }))
}

fn check_repeat_max(id: &str, max: &str, v: &mut Validator<'_>) {
    if max.contains("{{") {
        v.check_template(id, max, false);
    } else if max.trim().parse::<usize>().map_or(true, |n| n == 0) {
        v.error(id, format!("repeat.max must be a positive integer (got '{max}')"));
    }
}

#[cfg(test)]
mod tests;
