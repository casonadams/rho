use std::collections::BTreeMap;

pub const RESERVED_SCOPE_NAMES: [&str; 3] = ["inputs", "item", "round"];
pub const DEFAULT_CONCURRENCY: usize = 4;
pub const DEFAULT_TIMEOUT_SEC: u64 = 1_800;

#[derive(Debug, Clone, PartialEq)]
pub struct Workflow {
    pub name: String,
    pub description: String,
    pub isolation: Isolation,
    pub inputs: Vec<InputSpec>,
    pub steps: Vec<Step>,
    pub outputs: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Isolation {
    #[default]
    None,
    Branch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputSpec {
    pub name: String,
    pub description: Option<String>,
    pub choices: Vec<String>,
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub id: String,
    pub kind: StepKind,
    pub over: Option<String>,
    pub concurrency: usize,
    pub context: Vec<String>,
    pub when: Option<String>,
    pub expected_output: Option<String>,
    pub output: OutputFormat,
    pub check: Option<Check>,
    pub timeout_sec: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepKind {
    Agent(AgentStep),
    Run {
        command: String,
    },
    Builtin {
        builtin: Builtin,
        with: BTreeMap<String, String>,
    },
    Workflow(WorkflowCall),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentStep {
    pub agent: String,
    pub prompt: String,
    pub max_turns: Option<usize>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowCall {
    pub name: String,
    pub with: BTreeMap<String, String>,
    pub repeat: Option<Repeat>,
    pub carry: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repeat {
    pub until_empty: String,
    pub max: String,
    pub stop_on_no_progress: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    RepoScopes,
    GitCommit,
}

impl Builtin {
    pub const ALL: [(&'static str, Builtin); 2] =
        [("repo_scopes", Builtin::RepoScopes), ("git_commit", Builtin::GitCommit)];

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|(_, b)| *b == self).map_or("unknown", |(n, _)| n)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub command: String,
    pub retries: usize,
    pub on_fail: OnFail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnFail {
    #[default]
    Stop,
    Stash,
    Continue,
}

impl Workflow {
    #[must_use]
    pub fn argument_hint(&self) -> String {
        self.inputs
            .iter()
            .map(|input| match (input.choices.is_empty(), input.default.is_some()) {
                (false, _) => format!("<{}>", input.choices.join("|")),
                (true, true) => format!("[{}]", input.name),
                (true, false) => format!("<{}>", input.name),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn bind_inputs(&self, args: &[&str]) -> Result<serde_json::Map<String, serde_json::Value>, String> {
        let mut provided = serde_json::Map::new();
        let mut positional = Vec::new();
        for arg in args {
            match arg.split_once('=') {
                Some((key, value)) if self.inputs.iter().any(|i| i.name == key) => {
                    provided.insert(key.to_string(), serde_json::Value::String(value.to_string()));
                }
                _ => positional.push(*arg),
            }
        }
        let open: Vec<&InputSpec> = self.inputs.iter().filter(|i| !provided.contains_key(&i.name)).collect();
        if open.is_empty() && !positional.is_empty() {
            return Err(format!("too many arguments for workflow '{}'", self.name));
        }
        for (index, input) in open.iter().enumerate() {
            let value = if index + 1 == open.len() {
                positional.get(index..).map(|rest| rest.join(" "))
            } else {
                positional.get(index).map(|v| (*v).to_string())
            };
            if let Some(value) = value.filter(|v| !v.is_empty()) {
                provided.insert(input.name.clone(), serde_json::Value::String(value));
            }
        }
        self.resolve_inputs(provided)
    }

    pub fn resolve_inputs(
        &self,
        mut provided: serde_json::Map<String, serde_json::Value>,
    ) -> Result<serde_json::Map<String, serde_json::Value>, String> {
        if let Some(unknown) = provided.keys().find(|k| !self.inputs.iter().any(|i| &i.name == *k)) {
            return Err(format!("workflow '{}' has no input named '{unknown}'", self.name));
        }
        let mut resolved = serde_json::Map::new();
        for input in &self.inputs {
            let value = provided
                .remove(&input.name)
                .or_else(|| input.default.clone())
                .ok_or_else(|| format!("workflow '{}' requires input '{}'", self.name, input.name))?;
            check_choice(input, &value)?;
            resolved.insert(input.name.clone(), value);
        }
        Ok(resolved)
    }
}

fn check_choice(input: &InputSpec, value: &serde_json::Value) -> Result<(), String> {
    if input.choices.is_empty() {
        return Ok(());
    }
    let text = value.as_str().unwrap_or_default();
    if input.choices.iter().any(|c| c == text) {
        return Ok(());
    }
    Err(format!(
        "input '{}' must be one of: {} (got '{}')",
        input.name,
        input.choices.join(", "),
        crate::workflow::template::value_to_text(value)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn workflow() -> Workflow {
        Workflow {
            name: "refine".into(),
            description: String::new(),
            isolation: Isolation::None,
            inputs: vec![
                InputSpec {
                    name: "profile".into(),
                    description: None,
                    choices: vec!["review".into(), "security".into()],
                    default: None,
                },
                InputSpec {
                    name: "max_rounds".into(),
                    description: None,
                    choices: Vec::new(),
                    default: Some(json!(6)),
                },
            ],
            steps: Vec::new(),
            outputs: BTreeMap::new(),
        }
    }

    #[test]
    fn binds_positional_named_and_default_inputs() {
        let cases: [(&[&str], serde_json::Value); 3] = [
            (&["security"], json!({"profile": "security", "max_rounds": 6})),
            (&["review", "2"], json!({"profile": "review", "max_rounds": "2"})),
            (
                &["max_rounds=3", "security"],
                json!({"profile": "security", "max_rounds": "3"}),
            ),
        ];
        for (args, expected) in cases {
            let bound = workflow().bind_inputs(args).unwrap();
            assert_eq!(serde_json::Value::Object(bound), expected, "{args:?}");
        }
    }

    #[test]
    fn rejects_missing_unknown_and_invalid_choice_inputs() {
        let wf = workflow();
        assert!(wf.bind_inputs(&[]).unwrap_err().contains("requires input 'profile'"));
        assert!(wf.bind_inputs(&["fuzz"]).unwrap_err().contains("must be one of"));
        let mut unknown = serde_json::Map::new();
        unknown.insert("nope".into(), json!("x"));
        assert!(
            wf.resolve_inputs(unknown)
                .unwrap_err()
                .contains("no input named 'nope'")
        );
    }

    #[test]
    fn argument_hint_lists_choices_and_optional_inputs() {
        assert_eq!(workflow().argument_hint(), "<review|security> [max_rounds]");
    }
}
