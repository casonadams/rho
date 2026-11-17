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

pub(crate) fn workflow_prompt(name: &str, inputs: &serde_json::Map<String, serde_json::Value>) -> String {
    let args = serde_json::json!({ "name": name, "inputs": inputs });
    format!(
        "Run the `{name}` workflow now: call the `workflow` tool exactly once with these arguments, then summarize its report for me.\n\n```json\n{args}\n```"
    )
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
        out.push_str("  (none)\n");
    }
    for bundle in &catalog.bundles {
        let usage = format!("/{} {}", bundle.name, bundle.entry.argument_hint());
        let _ = writeln!(
            out,
            "  {:<44} {} [{}]",
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

pub(crate) async fn handle_workflows(ctx: &SlashCommandContext<'_>) -> CommandResult {
    let catalog = discover_workflows(std::env::current_dir().ok(), context_home(ctx)).await;
    ctx.renderer.print_notice(&format_workflow_list(&catalog));
    CommandResult::Continue
}

pub(crate) async fn try_handle_workflow(
    ctx: &SlashCommandContext<'_>,
    name: &str,
    parts: &[&str],
) -> Option<CommandResult> {
    let catalog = discover_workflows(std::env::current_dir().ok(), context_home(ctx)).await;
    let bundle = catalog.find(name)?;
    match bundle.entry.bind_inputs(parts.get(1..).unwrap_or_default()) {
        Ok(inputs) => Some(CommandResult::ExpandedPrompt {
            text: workflow_prompt(name, &inputs),
        }),
        Err(error) => {
            ctx.renderer.print_notice(&format!(
                "  /{name}: {error}\n  Usage: /{name} {}\n",
                bundle.entry.argument_hint()
            ));
            Some(CommandResult::Continue)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_embeds_exact_tool_arguments() {
        let mut inputs = serde_json::Map::new();
        inputs.insert("profile".into(), serde_json::json!("security"));
        let prompt = workflow_prompt("refine", &inputs);
        assert!(prompt.starts_with("Run the `refine` workflow now: call the `workflow` tool exactly once"));
        let block = prompt
            .split("```json\n")
            .nth(1)
            .and_then(|rest| rest.split("\n```").next())
            .unwrap();
        let args: serde_json::Value = serde_json::from_str(block).unwrap();
        assert_eq!(
            args,
            serde_json::json!({"name": "refine", "inputs": {"profile": "security"}})
        );
    }

    #[test]
    fn list_shows_usage_origin_and_invalid_workflows() {
        let mut catalog = WorkflowCatalog::discover(None, None);
        catalog
            .errors
            .push("workflow 'broken' is invalid:\n- step 'a': oops".to_string());
        let text = format_workflow_list(&catalog);
        assert!(text.contains("/refine <review|security|performance|reliability|architecture> [max_rounds] [verify]"));
        assert!(text.contains("[builtin]"));
        assert!(text.contains("Invalid workflows\n  workflow 'broken' is invalid:\n  - step 'a': oops"));
    }
}
