use super::builtin::BUILTINS;
use super::parse::parse_workflow;
use super::types::{Node, NodeKind, Workflow};
use crate::agents::{AgentMetadata, parse_agent_content};
use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

const ENTRY_FILE: &str = "workflow.xml";
const EXTENSION: &str = "xml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowOrigin {
    Builtin,
    User,
    Project,
}

fn is_contained(rel: &str) -> bool {
    !rel.is_empty()
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn read_relative(dir: &Path, rel: &str) -> Result<String, String> {
    if !is_contained(rel) {
        return Err("path must be relative and stay inside the workflow directory".to_string());
    }
    std::fs::read_to_string(dir.join(rel)).map_err(|e| e.to_string())
}

fn files_with_extension(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == ext))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("workflow")
        .to_string()
}

fn load_file(path: &Path, default_name: &str, dir: &Path) -> Result<Workflow, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_workflow(default_name, &text, &|rel| read_relative(dir, rel))
}

#[derive(Debug, Clone)]
pub struct WorkflowBundle {
    pub name: String,
    pub origin: WorkflowOrigin,
    pub location: PathBuf,
    pub dir: PathBuf,
    pub entry: Workflow,
    pub workflows: BTreeMap<String, Workflow>,
    pub agents: Vec<AgentMetadata>,
}

impl WorkflowBundle {
    fn load_dir(dir: &Path, origin: WorkflowOrigin) -> Result<Self, String> {
        let name = stem(dir);
        let entry = load_file(&dir.join(ENTRY_FILE), &name, dir)?;
        let mut workflows = BTreeMap::new();
        for path in files_with_extension(dir, EXTENSION) {
            if path.file_name().is_some_and(|f| f == ENTRY_FILE) {
                continue;
            }
            let local = load_file(&path, &stem(&path), dir)?;
            workflows.insert(local.name.clone(), local);
        }
        let agents = files_with_extension(&dir.join("agents"), "md")
            .iter()
            .map(|path| {
                let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
                Ok(parse_agent_content(&text, &stem(path), &path.display().to_string()))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            name: entry.name.clone(),
            origin,
            location: dir.join(ENTRY_FILE),
            dir: dir.to_path_buf(),
            entry,
            workflows,
            agents,
        })
    }

    fn load_embedded(name: &str, files: &[(&str, &str)]) -> Result<Self, String> {
        let read = |rel: &str| -> Result<String, String> {
            if !is_contained(rel) {
                return Err("path must be relative and stay inside the workflow directory".to_string());
            }
            let rel = rel.strip_prefix("./").unwrap_or(rel);
            files
                .iter()
                .find(|(path, _)| *path == rel)
                .map(|(_, text)| (*text).to_string())
                .ok_or_else(|| format!("builtin workflow '{name}' has no file {rel}"))
        };
        let dir = PathBuf::from(format!("builtin:{name}"));
        let entry = parse_workflow(name, &read(ENTRY_FILE)?, &read)?;
        let mut workflows = BTreeMap::new();
        let mut agents = Vec::new();
        for (path, text) in files {
            let file = Path::new(path);
            let nested = path.contains('/');
            if !nested && *path != ENTRY_FILE && file.extension().is_some_and(|e| e == EXTENSION) {
                let local = parse_workflow(&stem(file), text, &read)?;
                workflows.insert(local.name.clone(), local);
            } else if path.starts_with("agents/") && file.extension().is_some_and(|e| e == "md") {
                agents.push(parse_agent_content(
                    text,
                    &stem(file),
                    &dir.join(path).display().to_string(),
                ));
            }
        }
        Ok(Self {
            name: entry.name.clone(),
            origin: WorkflowOrigin::Builtin,
            location: dir.join(ENTRY_FILE),
            dir,
            entry,
            workflows,
            agents,
        })
    }

    fn load_single(path: &Path, origin: WorkflowOrigin) -> Result<Self, String> {
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let entry = load_file(path, &stem(path), &dir)?;
        Ok(Self {
            name: entry.name.clone(),
            origin,
            location: path.to_path_buf(),
            dir,
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
        catalog.add_builtins();
        if let Some(home) = home_dir {
            catalog.scan_dir(&home.join(".agents/workflows"), WorkflowOrigin::User);
        }
        if let Some(project) = project_dir.filter(|p| Some(*p) != home_dir) {
            catalog.scan_dir(&project.join(".agents/workflows"), WorkflowOrigin::Project);
        }
        catalog.validate_calls();
        catalog
    }

    fn add_builtins(&mut self) {
        for (name, files) in BUILTINS {
            match WorkflowBundle::load_embedded(name, files) {
                Ok(bundle) => self.bundles.push(bundle),
                Err(e) => self.errors.push(format!("builtin workflow '{name}': {e}")),
            }
        }
    }

    fn scan_dir(&mut self, dir: &Path, origin: WorkflowOrigin) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let loaded = if path.join(ENTRY_FILE).is_file() {
                WorkflowBundle::load_dir(&path, origin)
            } else if path.is_file() && path.extension().is_some_and(|e| e == EXTENSION) {
                WorkflowBundle::load_single(&path, origin)
            } else {
                continue;
            };
            match loaded {
                Ok(bundle) => {
                    self.bundles.retain(|b| b.name != bundle.name);
                    self.bundles.push(bundle);
                }
                Err(e) => self.errors.push(e),
            }
        }
    }

    #[must_use]
    pub fn find(&self, name: &str) -> Option<&WorkflowBundle> {
        self.bundles.iter().find(|b| b.name == name)
    }

    /// Resolves a `<call workflow>` name: the calling bundle's own files first, then other workflows.
    #[must_use]
    pub fn resolve_call<'a>(
        &'a self,
        bundle: &'a WorkflowBundle,
        name: &str,
    ) -> Option<(&'a WorkflowBundle, &'a Workflow)> {
        if let Some(local) = bundle.workflows.get(name) {
            return Some((bundle, local));
        }
        self.find(name).map(|other| (other, &other.entry))
    }

    fn validate_calls(&mut self) {
        let mut invalid = Vec::new();
        for bundle in &self.bundles {
            let mut problems = Vec::new();
            for wf in std::iter::once(&bundle.entry).chain(bundle.workflows.values()) {
                self.call_problems(bundle, wf, &wf.body, &mut problems);
                if let Some(cycle) = self.call_cycle(bundle, wf) {
                    problems.push(format!(
                        "{}: workflows call each other in a cycle ({})",
                        wf.name,
                        cycle.join(" -> ")
                    ));
                }
            }
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

    /// Returns the call chain leading from `start` back to itself, if any (direct self-calls are reported separately).
    fn call_cycle(&self, bundle: &WorkflowBundle, start: &Workflow) -> Option<Vec<String>> {
        let mut seen: HashSet<(&str, &str)> = HashSet::new();
        let mut stack = vec![(bundle, start, vec![start.name.clone()])];
        while let Some((owner, wf, path)) = stack.pop() {
            let mut targets = Vec::new();
            self.collect_call_targets(owner, &wf.body, &mut targets);
            for (next_owner, target) in targets {
                if target.name == wf.name {
                    continue;
                }
                let mut next_path = path.clone();
                next_path.push(target.name.clone());
                if std::ptr::eq(target, start) {
                    return Some(next_path);
                }
                if seen.insert((next_owner.name.as_str(), target.name.as_str())) {
                    stack.push((next_owner, target, next_path));
                }
            }
        }
        None
    }

    fn collect_call_targets<'a>(
        &'a self,
        bundle: &'a WorkflowBundle,
        nodes: &[Node],
        out: &mut Vec<(&'a WorkflowBundle, &'a Workflow)>,
    ) {
        for node in nodes {
            if let NodeKind::Call(call) = &node.kind
                && let Some(resolved) = self.resolve_call(bundle, &call.workflow)
            {
                out.push(resolved);
            }
            for body in node.kind.bodies() {
                self.collect_call_targets(bundle, body, out);
            }
        }
    }

    fn call_problems(&self, bundle: &WorkflowBundle, wf: &Workflow, nodes: &[Node], out: &mut Vec<String>) {
        for node in nodes {
            if let NodeKind::Call(call) = &node.kind {
                let at = format!("{} line {}", wf.name, node.line);
                match self.resolve_call(bundle, &call.workflow) {
                    None => out.push(format!("{at}: unknown workflow '{}'", call.workflow)),
                    Some((_, target)) if target.name == wf.name => {
                        out.push(format!("{at}: a workflow cannot call itself"));
                    }
                    Some((_, target)) => out.extend(
                        call.with
                            .keys()
                            .filter(|k| !target.inputs.iter().any(|i| &i.name == *k))
                            .map(|k| format!("{at}: '{}' has no input '{k}'", target.name)),
                    ),
                }
            }
            for body in node.kind.bodies() {
                self.call_problems(bundle, wf, body, out);
            }
        }
    }
}

#[cfg(test)]
mod tests;
