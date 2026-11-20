use super::template::references;
use super::types::{
    AgentNode, CallNode, ChooseNode, DEFAULT_TURNS, EachNode, InputSpec, Items, LoopNode, ModelRole, Node, NodeKind,
    OutputFormat, RunNode, Workflow,
};
use super::xml::{Element, parse_document};
use std::collections::{BTreeMap, BTreeSet};

pub type FileReader<'a> = &'a dyn Fn(&str) -> Result<String, String>;

const COMMON_ATTRS: [&str; 3] = ["id", "when", "unless"];

struct Builder<'a> {
    errors: Vec<String>,
    read_file: FileReader<'a>,
}

impl Builder<'_> {
    fn error(&mut self, el: &Element, message: impl std::fmt::Display) {
        self.errors.push(format!("line {}: <{}>: {message}", el.line, el.name));
    }

    fn check_attrs(&mut self, el: &Element, allowed: &[&str]) {
        for (key, _) in &el.attrs {
            if !allowed.contains(&key.as_str()) && !COMMON_ATTRS.contains(&key.as_str()) {
                self.error(el, format!("unknown attribute '{key}'"));
            }
        }
    }

    fn required(&mut self, el: &Element, attr: &str) -> Option<String> {
        let value = el.attr(attr).map(str::to_string);
        if value.is_none() {
            self.error(el, format!("missing required attribute '{attr}'"));
        }
        value
    }

    fn positive(&mut self, el: &Element, attr: &str, default: usize) -> usize {
        let Some(raw) = el.attr(attr) else {
            return default;
        };
        match raw.trim().parse::<usize>() {
            Ok(n) if n > 0 => n,
            _ => {
                self.error(el, format!("'{attr}' must be a positive integer (got '{raw}')"));
                default
            }
        }
    }

    fn output(&mut self, el: &Element) -> OutputFormat {
        match el.attr("output") {
            None | Some("text") => OutputFormat::Text,
            Some("json") => OutputFormat::Json,
            Some(other) => {
                self.error(el, format!("output must be 'text' or 'json' (got '{other}')"));
                OutputFormat::Text
            }
        }
    }

    fn nodes(&mut self, children: &[Element]) -> Vec<Node> {
        children.iter().filter_map(|child| self.node(child)).collect()
    }

    fn node(&mut self, el: &Element) -> Option<Node> {
        let kind = match el.name.as_str() {
            "agent" => self.agent(el).map(NodeKind::Agent),
            "run" => self.run(el).map(NodeKind::Run),
            "call" => self.call(el).map(NodeKind::Call),
            "sequence" => self.group(el).map(NodeKind::Sequence),
            "parallel" => self.group(el).map(NodeKind::Parallel),
            "each" => self.each(el).map(NodeKind::Each),
            "loop" => self.loop_node(el).map(NodeKind::Loop),
            "choose" => self.choose(el).map(NodeKind::Choose),
            other => {
                self.error(el, format!("unknown element '{other}'"));
                None
            }
        }?;
        Some(Node {
            id: el.attr("id").map(str::to_string),
            when: el.attr("when").map(str::to_string),
            unless: el.attr("unless").map(str::to_string),
            kind,
            line: el.line,
        })
    }

    fn prompt(&mut self, el: &Element) -> Option<String> {
        let inline = el.text.as_deref().map(dedent).unwrap_or_default();
        match (el.attr("prompt-file"), inline.is_empty()) {
            (Some(path), true) => match (self.read_file)(path) {
                Ok(text) => Some(text.trim().to_string()),
                Err(e) => {
                    self.error(el, format!("cannot read prompt-file '{path}': {e}"));
                    None
                }
            },
            (Some(_), false) => {
                self.error(el, "use either a prompt-file or inline text, not both");
                None
            }
            (None, true) => {
                self.error(el, "needs a prompt (inline text or prompt-file)");
                None
            }
            (None, false) => Some(inline),
        }
    }

    fn agent(&mut self, el: &Element) -> Option<AgentNode> {
        self.check_attrs(
            el,
            &["use", "model", "persona", "turns", "output", "context", "prompt-file"],
        );
        let agent = self.required(el, "use");
        let model = el.attr("model").map(ModelRole::parse).transpose().unwrap_or_else(|e| {
            self.error(el, e);
            None
        });
        let turns = self.positive(el, "turns", DEFAULT_TURNS);
        let output = self.output(el);
        let prompt = self.prompt(el);
        let context = el.attr("context").map(split_list).unwrap_or_default();
        Some(AgentNode {
            agent: agent?,
            prompt: prompt?,
            model,
            persona: el
                .attr("persona")
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string),
            turns,
            output,
            context,
        })
    }

    fn run(&mut self, el: &Element) -> Option<RunNode> {
        self.check_attrs(el, &["cmd", "output"]);
        let cmd = self.required(el, "cmd")?;
        let output = self.output(el);
        match split_command(&cmd) {
            Ok(args) => Some(RunNode { args, output }),
            Err(e) => {
                self.error(el, e);
                None
            }
        }
    }

    fn call(&mut self, el: &Element) -> Option<CallNode> {
        let workflow = self.required(el, "workflow")?;
        let with = el
            .attrs
            .iter()
            .filter(|(k, _)| k != "workflow" && !COMMON_ATTRS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Some(CallNode { workflow, with })
    }

    fn group(&mut self, el: &Element) -> Option<Vec<Node>> {
        self.check_attrs(el, &[]);
        self.block(el)
    }

    fn block(&mut self, el: &Element) -> Option<Vec<Node>> {
        if el.children.is_empty() {
            self.error(el, "needs at least one step");
        }
        Some(self.nodes(&el.children))
    }

    fn each(&mut self, el: &Element) -> Option<EachNode> {
        self.check_attrs(el, &["items", "as", "parallel"]);
        let items = self.required(el, "items")?;
        let items = if items.trim().starts_with("{{") {
            Items::Reference(items)
        } else {
            Items::Literal(split_list(&items))
        };
        let parallel = self.positive(el, "parallel", 1);
        let body = self.block(el)?;
        Some(EachNode {
            items,
            var: el.attr("as").unwrap_or("item").to_string(),
            parallel,
            body,
        })
    }

    fn loop_node(&mut self, el: &Element) -> Option<LoopNode> {
        self.check_attrs(el, &["max", "while", "until", "stop-if-unchanged"]);
        let max = self.required(el, "max")?;
        if !max.contains("{{") && max.trim().parse::<usize>().map_or(true, |n| n == 0) {
            self.error(
                el,
                format!("'max' must be a positive integer or a reference (got '{max}')"),
            );
        }
        let body = self.block(el)?;
        Some(LoopNode {
            max,
            while_: el.attr("while").map(str::to_string),
            until: el.attr("until").map(str::to_string),
            stop_if_unchanged: el.attr("stop-if-unchanged").map(str::to_string),
            body,
        })
    }

    fn choose(&mut self, el: &Element) -> Option<ChooseNode> {
        self.check_attrs(el, &["on"]);
        let on = self.required(el, "on")?;
        let mut cases = Vec::new();
        let mut otherwise = None;
        for child in &el.children {
            match child.name.as_str() {
                "case" => {
                    self.check_attrs(child, &["value"]);
                    let value = self.required(child, "value");
                    let body = self.block(child);
                    if let (Some(value), Some(body)) = (value, body) {
                        cases.push((value, body));
                    }
                }
                "otherwise" if otherwise.is_none() => otherwise = self.group(child),
                _ => self.error(child, "only <case> and one <otherwise> are allowed inside <choose>"),
            }
        }
        Some(ChooseNode { on, cases, otherwise })
    }
}

fn split_list(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(str::to_string).collect()
}

pub fn split_command(cmd: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_arg = false;
    let mut quote: Option<char> = None;
    for c in cmd.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                in_arg = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_arg {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                }
            }
            (None, c) => {
                current.push(c);
                in_arg = true;
            }
        }
    }
    if quote.is_some() {
        return Err(format!("unterminated quote in cmd: {cmd}"));
    }
    if in_arg {
        args.push(current);
    }
    if args.is_empty() {
        return Err("cmd is empty".to_string());
    }
    Ok(args)
}

fn dedent(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.iter().position(|l| !l.trim().is_empty());
    let last = lines.iter().rposition(|l| !l.trim().is_empty());
    let (Some(first), Some(last)) = (first, last) else {
        return String::new();
    };
    let body = &lines[first..=last];
    let indent = body
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    body.iter()
        .map(|l| l.get(indent..).unwrap_or_else(|| l.trim_start()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_inputs(root: &Element, b: &mut Builder<'_>) -> Vec<InputSpec> {
    let mut inputs: Vec<InputSpec> = Vec::new();
    for el in root.children.iter().filter(|c| c.name == "input") {
        b.check_attrs(el, &["name", "choices", "default", "description"]);
        let Some(name) = b.required(el, "name") else {
            continue;
        };
        if inputs.iter().any(|i| i.name == name) {
            b.error(el, format!("duplicate input '{name}'"));
        }
        inputs.push(InputSpec {
            name,
            description: el.attr("description").map(str::to_string),
            choices: el.attr("choices").map(split_list).unwrap_or_default(),
            default: el.attr("default").map(str::to_string),
        });
    }
    inputs
}

fn parse_outputs(root: &Element, b: &mut Builder<'_>) -> Vec<(String, String)> {
    let mut outputs = Vec::new();
    for el in root.children.iter().filter(|c| c.name == "output") {
        b.check_attrs(el, &["name", "value"]);
        if let (Some(name), Some(value)) = (b.required(el, "name"), b.required(el, "value")) {
            outputs.push((name, value));
        }
    }
    outputs
}

fn reject_isolation(root: &Element, b: &mut Builder<'_>) {
    if root.attr("isolation").is_some() {
        b.error(
            root,
            "the isolation attribute was removed: every workflow now runs in its own git worktree; delete the attribute".to_string(),
        );
    }
}

pub fn parse_workflow(default_name: &str, text: &str, read_file: FileReader<'_>) -> Result<Workflow, String> {
    let root = parse_document(text, &["agent"]).map_err(|e| format!("workflow '{default_name}': {e}"))?;
    let name = root.attr("name").unwrap_or(default_name).to_string();
    if root.name != "workflow" {
        return Err(format!(
            "workflow '{name}': the root element must be <workflow>, not <{}>",
            root.name
        ));
    }
    let mut b = Builder {
        errors: Vec::new(),
        read_file,
    };
    b.check_attrs(&root, &["name", "description", "isolation"]);
    reject_isolation(&root, &mut b);
    let inputs = parse_inputs(&root, &mut b);
    let outputs = parse_outputs(&root, &mut b);
    let steps: Vec<Element> = root
        .children
        .iter()
        .filter(|c| c.name != "input" && c.name != "output")
        .cloned()
        .collect();
    let body = b.nodes(&steps);
    if steps.is_empty() {
        b.errors.push("workflow has no steps".to_string());
    }
    let workflow = Workflow {
        name: name.clone(),
        description: root.attr("description").unwrap_or_default().to_string(),
        inputs,
        body,
        outputs,
    };
    let mut errors = b.errors;
    errors.extend(validate(&workflow));
    if errors.is_empty() {
        Ok(workflow)
    } else {
        Err(format!("workflow '{name}' is invalid:\n- {}", errors.join("\n- ")))
    }
}

struct Checker {
    names: BTreeSet<String>,
    ids: BTreeSet<String>,
    vars: Vec<String>,
    errors: Vec<String>,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn collect_ids(nodes: &[Node], into: &mut Vec<(String, usize)>) {
    for node in nodes {
        if let Some(id) = &node.id {
            into.push((id.clone(), node.line));
        }
        for body in node.kind.bodies() {
            collect_ids(body, into);
        }
    }
}

impl Checker {
    fn template(&mut self, line: usize, template: &str) {
        match references(template) {
            Ok(paths) => {
                for path in paths {
                    let root = path.split('.').next().unwrap_or_default();
                    if !self.names.contains(root) && !self.vars.iter().any(|v| v == root) {
                        self.errors
                            .push(format!("line {line}: unknown reference '{{{{{path}}}}}'"));
                    }
                }
            }
            Err(e) => self.errors.push(format!("line {line}: {e}")),
        }
    }

    fn templates<'a>(&mut self, line: usize, templates: impl IntoIterator<Item = &'a String>) {
        for t in templates {
            self.template(line, t);
        }
    }

    fn nodes(&mut self, nodes: &[Node]) {
        for node in nodes {
            self.node(node);
        }
    }

    fn node(&mut self, node: &Node) {
        self.templates(node.line, node.when.iter().chain(&node.unless));
        match &node.kind {
            NodeKind::Agent(agent) => {
                self.template(node.line, &agent.prompt);
                for id in &agent.context {
                    if !self.ids.contains(id) {
                        self.errors
                            .push(format!("line {}: context '{id}' is not a step id", node.line));
                    }
                }
            }
            NodeKind::Run(run) => self.templates(node.line, &run.args),
            NodeKind::Call(call) => self.templates(node.line, call.with.values()),
            NodeKind::Sequence(body) | NodeKind::Parallel(body) => self.nodes(body),
            NodeKind::Each(each) => self.each(node.line, each),
            NodeKind::Loop(l) => {
                self.templates(
                    node.line,
                    std::iter::once(&l.max)
                        .chain(&l.while_)
                        .chain(&l.until)
                        .chain(&l.stop_if_unchanged),
                );
                self.nodes(&l.body);
            }
            NodeKind::Choose(c) => {
                self.template(node.line, &c.on);
                for body in node.kind.bodies() {
                    self.nodes(body);
                }
            }
        }
    }

    fn each(&mut self, line: usize, each: &EachNode) {
        if let Items::Reference(r) = &each.items {
            self.template(line, r);
        }
        if !valid_name(&each.var) || self.names.contains(&each.var) || self.vars.contains(&each.var) {
            self.errors.push(format!(
                "line {line}: 'as' name '{}' is invalid or already used",
                each.var
            ));
        }
        self.vars.push(each.var.clone());
        self.nodes(&each.body);
        self.vars.pop();
    }
}

fn validate(workflow: &Workflow) -> Vec<String> {
    let mut errors = Vec::new();
    let mut names: BTreeSet<String> = BTreeSet::new();
    for input in &workflow.inputs {
        if !valid_name(&input.name) {
            errors.push(format!("invalid input name '{}'", input.name));
        }
        names.insert(input.name.clone());
    }
    let mut ids = Vec::new();
    collect_ids(&workflow.body, &mut ids);
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for (id, line) in &ids {
        if !valid_name(id) {
            errors.push(format!(
                "line {line}: invalid id '{id}' (use letters, digits, '-', '_')"
            ));
        } else if names.contains(id) && !seen.contains_key(id) {
            errors.push(format!("line {line}: id '{id}' is already an input name"));
        } else if let Some(first) = seen.insert(id.clone(), *line) {
            errors.push(format!("line {line}: duplicate id '{id}' (first used on line {first})"));
        }
    }
    let ids: BTreeSet<String> = seen.into_keys().collect();
    names.extend(ids.iter().cloned());
    let mut checker = Checker {
        names,
        ids,
        vars: Vec::new(),
        errors,
    };
    checker.nodes(&workflow.body);
    for (name, value) in &workflow.outputs {
        let before = checker.errors.len();
        checker.template(0, value);
        for e in &mut checker.errors[before..] {
            *e = e.replacen("line 0", &format!("output '{name}'"), 1);
        }
    }
    checker.errors
}

#[cfg(test)]
mod tests;
