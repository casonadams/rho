use super::parse::parse_workflow;
use super::types::{StepKind, Workflow};
use crate::agents::{AgentMetadata, parse_agent_content};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

const ENTRY_FILE: &str = "workflow.toml";

type EmbeddedFiles = &'static [(&'static str, &'static str)];

const BUILTIN_BUNDLES: [(&str, EmbeddedFiles); 1] = [("refine", REFINE_FILES)];

const REFINE_FILES: EmbeddedFiles = &[
    ("workflow.toml", include_str!("builtin/refine/workflow.toml")),
    ("round.toml", include_str!("builtin/refine/round.toml")),
    (
        "agents/validator.md",
        include_str!("builtin/refine/agents/validator.md"),
    ),
    (
        "agents/implementor.md",
        include_str!("builtin/refine/agents/implementor.md"),
    ),
    ("prompts/scan.md", include_str!("builtin/refine/prompts/scan.md")),
    (
        "prompts/validate.md",
        include_str!("builtin/refine/prompts/validate.md"),
    ),
    (
        "prompts/implement.md",
        include_str!("builtin/refine/prompts/implement.md"),
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowOrigin {
    Builtin,
    User,
    Project,
}

#[derive(Debug, Clone)]
enum BundleSource {
    Dir(PathBuf),
    Embedded(EmbeddedFiles),
}

impl BundleSource {
    fn read(&self, rel: &str) -> Result<String, String> {
        if !is_contained(rel) {
            return Err("path must be relative and stay inside the workflow directory".to_string());
        }
        match self {
            Self::Dir(dir) => std::fs::read_to_string(dir.join(rel)).map_err(|e| e.to_string()),
            Self::Embedded(files) => files
                .iter()
                .find(|(path, _)| *path == rel)
                .map(|(_, text)| (*text).to_string())
                .ok_or_else(|| "not found".to_string()),
        }
    }

    fn files_with_extension(&self, subdir: &str, ext: &str) -> Vec<String> {
        let mut files: Vec<String> = match self {
            Self::Dir(dir) => std::fs::read_dir(dir.join(subdir))
                .map(|entries| {
                    entries
                        .flatten()
                        .filter_map(|e| e.file_name().into_string().ok())
                        .filter(|name| name.ends_with(ext))
                        .map(|name| join_rel(subdir, &name))
                        .collect()
                })
                .unwrap_or_default(),
            Self::Embedded(files) => files
                .iter()
                .map(|(path, _)| (*path).to_string())
                .filter(|path| parent_of(path) == subdir && path.ends_with(ext))
                .collect(),
        };
        files.sort();
        files
    }

    fn location(&self, name: &str) -> String {
        match self {
            Self::Dir(dir) => dir.display().to_string(),
            Self::Embedded(_) => format!("builtin:{name}"),
        }
    }
}

fn join_rel(subdir: &str, name: &str) -> String {
    if subdir.is_empty() {
        name.to_string()
    } else {
        format!("{subdir}/{name}")
    }
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn is_contained(rel: &str) -> bool {
    let path = Path::new(rel);
    !rel.is_empty()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn stem(rel: &str) -> &str {
    let file = rel.rsplit('/').next().unwrap_or(rel);
    file.rsplit_once('.').map_or(file, |(s, _)| s)
}

#[derive(Debug, Clone)]
pub struct WorkflowBundle {
    pub name: String,
    pub origin: WorkflowOrigin,
    pub location: String,
    pub entry: Workflow,
    pub workflows: BTreeMap<String, Workflow>,
    pub agents: Vec<AgentMetadata>,
}

impl WorkflowBundle {
    fn load(name: &str, origin: WorkflowOrigin, source: &BundleSource) -> Result<Self, String> {
        let reader = |rel: &str| source.read(rel);
        let mut workflows = BTreeMap::new();
        for rel in source.files_with_extension("", ".toml") {
            if rel == ENTRY_FILE {
                continue;
            }
            let text = source.read(&rel).map_err(|e| format!("{rel}: {e}"))?;
            let wf = parse_workflow(stem(&rel), &text, &reader)?;
            workflows.insert(stem(&rel).to_string(), wf);
        }
        let entry_text = source.read(ENTRY_FILE).map_err(|e| format!("{ENTRY_FILE}: {e}"))?;
        let entry = parse_workflow(name, &entry_text, &reader)?;
        let agents = source
            .files_with_extension("agents", ".md")
            .iter()
            .map(|rel| {
                let text = source.read(rel).map_err(|e| format!("{rel}: {e}"))?;
                let location = format!("{}/{rel}", source.location(name));
                Ok(parse_agent_content(&text, stem(rel), &location))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            name: entry.name.clone(),
            origin,
            location: source.location(name),
            entry,
            workflows,
            agents,
        })
    }

    fn load_single_file(path: &Path, origin: WorkflowOrigin) -> Result<Self, String> {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("workflow");
        let dir = path.parent().unwrap_or(Path::new("."));
        let source = BundleSource::Dir(dir.to_path_buf());
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let entry = parse_workflow(name, &text, &|rel| source.read(rel))?;
        Ok(Self {
            name: entry.name.clone(),
            origin,
            location: path.display().to_string(),
            entry,
            workflows: BTreeMap::new(),
            agents: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct WorkflowCatalog {
    pub bundles: Vec<WorkflowBundle>,
    pub errors: Vec<String>,
}

impl WorkflowCatalog {
    #[must_use]
    pub fn discover(project_dir: Option<&Path>, home_dir: Option<&Path>) -> Self {
        let mut catalog = Self::default();
        for (name, files) in BUILTIN_BUNDLES {
            catalog.add(
                name,
                WorkflowBundle::load(name, WorkflowOrigin::Builtin, &BundleSource::Embedded(files)),
            );
        }
        if let Some(home) = home_dir {
            catalog.scan_dir(&home.join(".agents/workflows"), WorkflowOrigin::User);
        }
        if let Some(project) = project_dir.filter(|p| Some(*p) != home_dir) {
            catalog.scan_dir(&project.join(".agents/workflows"), WorkflowOrigin::Project);
        }
        catalog.validate_calls();
        catalog
    }

    fn scan_dir(&mut self, dir: &Path, origin: WorkflowOrigin) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if path.join(ENTRY_FILE).is_file() {
                let source = BundleSource::Dir(path.clone());
                self.add(&name, WorkflowBundle::load(&name, origin, &source));
            } else if path.extension().is_some_and(|e| e == "toml") {
                self.add(&name, WorkflowBundle::load_single_file(&path, origin));
            }
        }
    }

    fn add(&mut self, name: &str, loaded: Result<WorkflowBundle, String>) {
        match loaded {
            Ok(bundle) => {
                self.bundles.retain(|b| b.name != bundle.name);
                self.bundles.push(bundle);
            }
            Err(e) => self.errors.push(format!("workflow '{name}': {e}")),
        }
    }

    #[must_use]
    pub fn find(&self, name: &str) -> Option<&WorkflowBundle> {
        self.bundles.iter().find(|b| b.name == name)
    }

    #[must_use]
    pub fn resolve_call<'a>(&'a self, bundle: &'a WorkflowBundle, name: &str) -> Option<&'a Workflow> {
        bundle.workflows.get(name).or_else(|| self.find(name).map(|b| &b.entry))
    }

    fn validate_calls(&mut self) {
        let mut invalid = Vec::new();
        for bundle in &self.bundles {
            let problems: Vec<String> = std::iter::once(&bundle.entry)
                .chain(bundle.workflows.values())
                .flat_map(|wf| self.call_problems(bundle, wf))
                .collect();
            if !problems.is_empty() {
                invalid.push((bundle.name.clone(), problems));
            }
        }
        for (name, problems) in invalid {
            self.bundles.retain(|b| b.name != name);
            self.errors
                .push(format!("workflow '{name}' is invalid:\n- {}", problems.join("\n- ")));
        }
    }

    fn call_problems(&self, bundle: &WorkflowBundle, wf: &Workflow) -> Vec<String> {
        let mut problems = Vec::new();
        for step in &wf.steps {
            let StepKind::Workflow(call) = &step.kind else {
                continue;
            };
            let at = format!("{} step '{}'", wf.name, step.id);
            if call.name == wf.name {
                problems.push(format!("{at}: a workflow cannot call itself"));
                continue;
            }
            let Some(target) = self.resolve_call(bundle, &call.name) else {
                problems.push(format!("{at}: unknown workflow '{}'", call.name));
                continue;
            };
            problems.extend(call_shape_problems(&at, call, target));
        }
        problems
    }
}

fn call_shape_problems(at: &str, call: &super::types::WorkflowCall, target: &Workflow) -> Vec<String> {
    let has_input = |name: &str| target.inputs.iter().any(|i| i.name == name);
    let mut problems: Vec<String> = call
        .with
        .keys()
        .filter(|k| !has_input(k))
        .map(|k| format!("{at}: '{}' has no input '{k}'", target.name))
        .collect();
    if let Some(repeat) = &call.repeat
        && !target.outputs.contains_key(&repeat.until_empty)
    {
        problems.push(format!(
            "{at}: repeat.until_empty '{}' is not an output of '{}'",
            repeat.until_empty, target.name
        ));
    }
    for carried in &call.carry {
        if !target.outputs.contains_key(carried) || !has_input(carried) {
            problems.push(format!(
                "{at}: carry '{carried}' must be both an input and an output of '{}'",
                target.name
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests;
