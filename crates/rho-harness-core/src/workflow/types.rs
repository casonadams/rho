use std::collections::BTreeMap;

pub const DEFAULT_TURNS: usize = 100;
pub const DEFAULT_PARALLEL: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct Workflow {
    pub name: String,
    pub description: String,
    pub inputs: Vec<InputSpec>,
    pub body: Vec<Node>,
    pub outputs: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputSpec {
    pub name: String,
    pub description: Option<String>,
    pub choices: Vec<String>,
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: Option<String>,
    pub when: Option<String>,
    pub unless: Option<String>,
    pub kind: NodeKind,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    Agent(AgentNode),
    Run(RunNode),
    Call(CallNode),
    Sequence(Vec<Node>),
    Parallel(Vec<Node>),
    Each(EachNode),
    Loop(LoopNode),
    Choose(ChooseNode),
}

impl NodeKind {
    #[must_use]
    pub fn bodies(&self) -> Vec<&Vec<Node>> {
        match self {
            Self::Sequence(body) | Self::Parallel(body) => vec![body],
            Self::Each(each) => vec![&each.body],
            Self::Loop(l) => vec![&l.body],
            Self::Choose(c) => c.cases.iter().map(|(_, b)| b).chain(c.otherwise.as_ref()).collect(),
            Self::Agent(_) | Self::Run(_) | Self::Call(_) => Vec::new(),
        }
    }

    #[must_use]
    pub fn element_name(&self) -> &'static str {
        match self {
            Self::Agent(_) => "agent",
            Self::Run(_) => "run",
            Self::Call(_) => "call",
            Self::Sequence(_) => "sequence",
            Self::Parallel(_) => "parallel",
            Self::Each(_) => "each",
            Self::Loop(_) => "loop",
            Self::Choose(_) => "choose",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelRole {
    Default,
    Smol,
    Slow,
}

impl ModelRole {
    pub const NAMES: [&'static str; 3] = ["default", "smol", "slow"];

    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "default" => Ok(Self::Default),
            "smol" => Ok(Self::Smol),
            "slow" => Ok(Self::Slow),
            other => Err(format!(
                "model must be one of {} (got '{other}')",
                Self::NAMES.join(", ")
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Smol => "smol",
            Self::Slow => "slow",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentNode {
    pub agent: String,
    pub prompt: String,
    pub model: Option<ModelRole>,
    pub persona: Option<String>,
    pub turns: usize,
    pub output: OutputFormat,
    pub context: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunNode {
    pub args: Vec<String>,
    pub output: OutputFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallNode {
    pub workflow: String,
    pub with: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Items {
    Reference(String),
    Literal(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct EachNode {
    pub items: Items,
    pub var: String,
    pub parallel: usize,
    pub body: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoopNode {
    pub max: String,
    pub while_: Option<String>,
    pub until: Option<String>,
    pub stop_if_unchanged: Option<String>,
    pub body: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChooseNode {
    pub on: String,
    pub cases: Vec<(String, Vec<Node>)>,
    pub otherwise: Option<Vec<Node>>,
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

    /// Maps positional and `key=value` arguments onto declared inputs, applying defaults and choices.
    pub fn bind_inputs(&self, args: &[&str]) -> Result<BTreeMap<String, String>, String> {
        let mut provided = BTreeMap::new();
        let mut positional = Vec::new();
        for arg in args {
            match arg.split_once('=') {
                Some((key, value)) if self.inputs.iter().any(|i| i.name == key) => {
                    provided.insert(key.to_string(), value.to_string());
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
                provided.insert(input.name.clone(), value);
            }
        }
        self.resolve_inputs(provided)
    }

    pub fn resolve_inputs(&self, mut provided: BTreeMap<String, String>) -> Result<BTreeMap<String, String>, String> {
        if let Some(unknown) = provided.keys().find(|k| !self.inputs.iter().any(|i| &i.name == *k)) {
            return Err(format!("workflow '{}' has no input named '{unknown}'", self.name));
        }
        let mut resolved = BTreeMap::new();
        for input in &self.inputs {
            let value = provided
                .remove(&input.name)
                .or_else(|| input.default.clone())
                .ok_or_else(|| format!("workflow '{}' requires input '{}'", self.name, input.name))?;
            if !input.choices.is_empty() && !input.choices.contains(&value) {
                return Err(format!(
                    "input '{}' must be one of: {} (got '{value}')",
                    input.name,
                    input.choices.join(", ")
                ));
            }
            resolved.insert(input.name.clone(), value);
        }
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow() -> Workflow {
        Workflow {
            name: "refine".into(),
            description: String::new(),
            inputs: vec![
                InputSpec {
                    name: "profile".into(),
                    description: None,
                    choices: vec!["review".into(), "security".into()],
                    default: None,
                },
                InputSpec {
                    name: "rounds".into(),
                    description: None,
                    choices: Vec::new(),
                    default: Some("6".into()),
                },
            ],
            body: Vec::new(),
            outputs: Vec::new(),
        }
    }

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn binds_positional_named_and_default_inputs() {
        let cases: [(&[&str], BTreeMap<String, String>); 3] = [
            (&["security"], map(&[("profile", "security"), ("rounds", "6")])),
            (&["review", "2"], map(&[("profile", "review"), ("rounds", "2")])),
            (
                &["rounds=3", "security"],
                map(&[("profile", "security"), ("rounds", "3")]),
            ),
        ];
        for (args, expected) in cases {
            assert_eq!(workflow().bind_inputs(args).unwrap(), expected, "{args:?}");
        }
    }

    #[test]
    fn rejects_missing_unknown_extra_and_invalid_choice_inputs() {
        let wf = workflow();
        assert!(wf.bind_inputs(&[]).unwrap_err().contains("requires input 'profile'"));
        assert!(wf.bind_inputs(&["fuzz"]).unwrap_err().contains("must be one of"));
        assert!(
            wf.resolve_inputs(map(&[("nope", "x")]))
                .unwrap_err()
                .contains("no input named 'nope'")
        );
        let all_named = wf.bind_inputs(&["profile=review", "rounds=1", "extra"]).unwrap_err();
        assert!(all_named.contains("too many arguments"));
    }

    #[test]
    fn argument_hint_lists_choices_and_optional_inputs() {
        assert_eq!(workflow().argument_hint(), "<review|security> [rounds]");
    }

    #[test]
    fn model_roles_accept_only_known_names() {
        assert_eq!(ModelRole::parse("slow"), Ok(ModelRole::Slow));
        assert!(ModelRole::parse("guard").unwrap_err().contains("default, smol, slow"));
        assert!(ModelRole::parse("claude/opus").is_err());
    }
}
