use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::types::{CommandResult, SlashCommandContext};
use rho_harness_core::workflow::{WorkflowBundle, WorkflowCatalog, WorkflowOrigin};

pub(crate) async fn discover_workflows(cwd: Option<PathBuf>, home: Option<PathBuf>) -> WorkflowCatalog {
    tokio::task::spawn_blocking(move || WorkflowCatalog::discover(cwd.as_deref(), home.as_deref()))
        .await
        .unwrap_or_default()
}

fn context_home(ctx: &SlashCommandContext<'_>) -> Option<PathBuf> {
    ctx.home_dir
        .map(Path::to_path_buf)
        .or_else(rho_harness_core::config::home_dir)
}

pub(crate) async fn completion_entries() -> Vec<(String, String)> {
    let catalog = discover_workflows(std::env::current_dir().ok(), rho_harness_core::config::home_dir()).await;
    catalog
        .bundles
        .iter()
        .map(|b| (b.name.clone(), command_description(b)))
        .collect()
}

fn command_description(bundle: &WorkflowBundle) -> String {
    let hint = bundle.entry.argument_hint();
    let description = if bundle.entry.description.is_empty() {
        "Run workflow"
    } else {
        bundle.entry.description.as_str()
    };
    format!("{description} {hint}").trim().to_string()
}

fn origin_label(origin: WorkflowOrigin) -> &'static str {
    match origin {
        WorkflowOrigin::Builtin => "builtin",
        WorkflowOrigin::User => "user",
        WorkflowOrigin::Project => "project",
    }
}

pub(crate) fn format_workflow_list(catalog: &WorkflowCatalog) -> String {
    let mut out = String::from("\nWorkflows\n");
    if catalog.bundles.is_empty() {
        out.push_str("  (none) add one under .agents/workflows/ (see docs/workflows.md)\n");
    }
    for bundle in &catalog.bundles {
        let usage = format!("/workflow {} {}", bundle.name, bundle.entry.argument_hint());
        let _ = writeln!(
            out,
            "  {:<52} {} [{}]",
            usage.trim(),
            bundle.entry.description,
            origin_label(bundle.origin)
        );
    }
    if !catalog.errors.is_empty() {
        out.push_str("\nInvalid workflows\n");
        for error in &catalog.errors {
            let _ = writeln!(out, "  {}", error.replace('\n', "\n  "));
        }
    }
    out
}

/// Splits a command line on whitespace, keeping `"..."` and `'...'` groups together; an unclosed quote
/// falls back to plain whitespace splitting.
pub(crate) fn split_args(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in line.chars() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if std::mem::take(&mut started) {
                    args.push(std::mem::take(&mut current));
                }
            }
            (None, c) => {
                current.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return line.split_whitespace().map(str::to_string).collect();
    }
    if started {
        args.push(current);
    }
    args
}

pub(crate) async fn handle_workflow(ctx: &SlashCommandContext<'_>, parts: &[&str]) -> CommandResult {
    let catalog = discover_workflows(std::env::current_dir().ok(), context_home(ctx)).await;
    let Some(name) = parts.get(1) else {
        ctx.renderer.print_notice(&format_workflow_list(&catalog));
        return CommandResult::Continue;
    };
    match bind(&catalog, name, parts.get(2..).unwrap_or_default()) {
        Ok(inputs) => CommandResult::RunWorkflow {
            name: (*name).to_string(),
            inputs,
        },
        Err(message) => {
            ctx.renderer.print_notice(&format!("  /workflow {name}: {message}\n"));
            CommandResult::Continue
        }
    }
}

fn bind(catalog: &WorkflowCatalog, name: &str, args: &[&str]) -> Result<BTreeMap<String, String>, String> {
    let Some(bundle) = catalog.find(name) else {
        let problem = catalog.errors.iter().find(|e| e.contains(&format!("'{name}'")));
        return Err(match problem {
            Some(problem) => problem.clone(),
            None => "unknown workflow (run /workflow to list them)".to_string(),
        });
    };
    bundle
        .entry
        .bind_inputs(args)
        .map_err(|e| format!("{e}\n  Usage: /workflow {name} {}", bundle.entry.argument_hint()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(dir: &Path) -> WorkflowCatalog {
        let wf = dir.join(".agents/workflows");
        std::fs::create_dir_all(&wf).unwrap();
        std::fs::write(
            wf.join("refine.xml"),
            "<workflow description=\"Scan and fix\"><input name=\"profile\" choices=\"review security\"/><run cmd=\"true\"/></workflow>",
        )
        .unwrap();
        std::fs::write(wf.join("broken.xml"), "<workflow><run/></workflow>").unwrap();
        WorkflowCatalog::discover(Some(dir), None)
    }

    #[test]
    fn binds_inputs_or_explains_usage_and_problems() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = catalog(dir.path());
        assert_eq!(bind(&catalog, "refine", &["security"]).unwrap()["profile"], "security");
        let bad = bind(&catalog, "refine", &["fuzz"]).unwrap_err();
        assert!(
            bad.contains("must be one of") && bad.contains("Usage: /workflow refine <review|security>"),
            "{bad}"
        );
        assert!(
            bind(&catalog, "broken", &[])
                .unwrap_err()
                .contains("missing required attribute 'cmd'")
        );
        assert!(bind(&catalog, "ghost", &[]).unwrap_err().contains("unknown workflow"));
    }

    #[test]
    fn split_args_keeps_quoted_groups_and_falls_back_on_an_unclosed_quote() {
        let cases = [
            (
                "workflow audit security,correctness . 1 6",
                vec!["workflow", "audit", "security,correctness", ".", "1", "6"],
            ),
            (
                "workflow audit \"a b\" 'c d' e",
                vec!["workflow", "audit", "a b", "c d", "e"],
            ),
            (
                "workflow audit paths=\"src lib\" x",
                vec!["workflow", "audit", "paths=src lib", "x"],
            ),
            ("workflow audit \"\" x", vec!["workflow", "audit", "", "x"]),
            ("workflow audit don't stop", vec!["workflow", "audit", "don't", "stop"]),
            ("  workflow   audit  ", vec!["workflow", "audit"]),
        ];
        for (line, expected) in cases {
            assert_eq!(split_args(line), expected, "{line}");
        }
    }

    #[test]
    fn the_builtin_audit_is_listed_valid_and_overridable() {
        let catalog = WorkflowCatalog::discover(None, None);
        assert!(catalog.errors.is_empty(), "{:?}", catalog.errors);
        let audit = catalog.find("audit").expect("builtin audit");
        assert_eq!(audit.origin, WorkflowOrigin::Builtin);
        assert_eq!(
            audit.entry.argument_hint(),
            "[categories] [paths] [passes] [max_tasks] [scope]"
        );
        assert_eq!(audit.agents.len(), 14);
        assert!(format_workflow_list(&catalog).contains("[builtin]"));
        let dir = tempfile::tempdir().unwrap();
        let wf = dir.path().join(".agents/workflows");
        std::fs::create_dir_all(&wf).unwrap();
        std::fs::write(
            wf.join("audit.xml"),
            "<workflow description=\"mine\"><run cmd=\"true\"/></workflow>",
        )
        .unwrap();
        let overridden = WorkflowCatalog::discover(Some(dir.path()), None);
        assert_eq!(overridden.find("audit").unwrap().entry.description, "mine");
    }

    #[test]
    fn the_repository_refine_example_is_valid() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let catalog = WorkflowCatalog::discover(Some(repo), None);
        assert!(catalog.errors.is_empty(), "{:?}", catalog.errors);
        let refine = catalog.find("refine").expect(".agents/workflows/refine");
        assert_eq!(
            refine.entry.argument_hint(),
            "<review|security|performance|reliability|architecture> [rounds]"
        );
        let names: Vec<&str> = refine.agents.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["implementor", "reviewer", "validator"]);
        let implementor = &refine.agents[0];
        assert!(!implementor.tools.as_ref().unwrap().contains(&"bash".to_string()));
        let reviewer = &refine.agents[1];
        assert!(!reviewer.tools.as_ref().unwrap().contains(&"edit".to_string()));
    }

    #[test]
    fn list_shows_usage_origin_and_invalid_workflows() {
        let dir = tempfile::tempdir().unwrap();
        let text = format_workflow_list(&catalog(dir.path()));
        assert!(text.contains("/workflow refine <review|security>"), "{text}");
        assert!(text.contains("Scan and fix [project]"), "{text}");
        assert!(
            text.contains("Invalid workflows\n  workflow 'broken' is invalid:\n  - line 1"),
            "{text}"
        );
        assert!(format_workflow_list(&WorkflowCatalog::default()).contains("(none)"));
    }
}
