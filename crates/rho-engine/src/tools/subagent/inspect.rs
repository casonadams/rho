use super::conversation::{
    FILE_VERSIONS_ARTIFACT, FILES_CHANGED_ARTIFACT, RESULT_ARTIFACT, TOOL_CALLS_ARTIFACT, WORKSPACE_CHANGES_ARTIFACT,
    artifact_data, artifact_text,
};
use super::evidence::{ToolOutcome, outcome_line};
use super::workspace::{Fingerprint, WorkspaceChange, stale_paths};
use rho_harness_core::a2a::{Role, Task};
use rho_harness_core::session::SubagentContext;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

const MAX_MESSAGE_CHARS: usize = 4_000;
const MAX_RESULT_CHARS: usize = 12_000;
const MAX_LISTED_CONTEXTS: usize = 30;
const MAX_TITLE_CHARS: usize = 80;

fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    let total = text.chars().count();
    if total <= max {
        return text.to_string();
    }
    format!(
        "{}\n[... {} more characters]",
        text.chars().take(max).collect::<String>(),
        total - max
    )
}

fn data<T: DeserializeOwned + Default>(task: &Task, name: &str) -> T {
    artifact_data(task, name)
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

fn settings_line(task: &Task) -> String {
    let field = |key: &str| task.metadata.as_ref().and_then(|m| m[key].as_str()).map(str::to_string);
    let agent = field("agent").unwrap_or_else(|| "scout".to_string());
    let mut line = format!("agent {agent}");
    for (key, label) in [("name", "name"), ("model", "model")] {
        if let Some(value) = field(key) {
            let _ = write!(line, ", {label} {value}");
        }
    }
    line
}

fn title(task: &Task) -> String {
    let first = task
        .history
        .iter()
        .find(|m| m.role == Role::User)
        .map(|m| m.text_content())
        .unwrap_or_default();
    let line = first.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    if line.chars().count() > MAX_TITLE_CHARS {
        format!("{}...", line.chars().take(MAX_TITLE_CHARS).collect::<String>())
    } else {
        line.to_string()
    }
}

fn push_messages(out: &mut String, task: &Task) {
    out.push_str("\nMessages:\n");
    for message in &task.history {
        let speaker = match message.role {
            Role::User => "parent",
            Role::Agent => "agent",
        };
        let _ = writeln!(out, "[{speaker}] {}", clip(&message.text_content(), MAX_MESSAGE_CHARS));
    }
}

async fn push_evidence(out: &mut String, task: &Task, root: &Path) {
    let log: Vec<ToolOutcome> = data(task, TOOL_CALLS_ARTIFACT);
    match outcome_line(&log) {
        Some(line) => {
            let _ = writeln!(out, "\n{line}");
            for outcome in &log {
                let _ = writeln!(out, "  {}", outcome.line());
            }
        }
        None => out.push_str("\nNo tool calls in this task.\n"),
    }
    let changed: Vec<String> = data(task, FILES_CHANGED_ARTIFACT);
    if !changed.is_empty() {
        let _ = writeln!(out, "Files changed: {}", changed.join(", "));
    }
    let workspace: Vec<WorkspaceChange> = data(task, WORKSPACE_CHANGES_ARTIFACT);
    if !workspace.is_empty() {
        let labels: Vec<String> = workspace.iter().map(WorkspaceChange::label).collect();
        let _ = writeln!(out, "Workspace changes during this task: {}", labels.join(", "));
    }
    let versions: BTreeMap<String, Fingerprint> = data(task, FILE_VERSIONS_ARTIFACT);
    if !versions.is_empty() {
        let names: Vec<&str> = versions.keys().map(String::as_str).collect();
        let _ = writeln!(out, "Files read or edited so far: {}", names.join(", "));
        let stale = stale_paths(root, &versions).await;
        if !stale.is_empty() {
            let _ = writeln!(out, "Changed since the agent last saw them: {}", stale.join(", "));
        }
    }
}

/// Everything recorded about one task, without running the subagent.
#[must_use]
pub(crate) async fn render_task(context: &SubagentContext, task_id: &str, root: &Path) -> Option<String> {
    let task = context.tasks.iter().find(|t| t.id == task_id)?;
    let position = context.tasks.iter().position(|t| t.id == task_id).unwrap_or_default();
    let mut out = format!(
        "Task `{}` \u{b7} {} \u{b7} {} (task {} of {} in this conversation)\n",
        task.id,
        task.status.state.label(),
        settings_line(task),
        position + 1,
        context.tasks.len()
    );
    let others: Vec<String> = context
        .tasks
        .iter()
        .filter(|t| t.id != task_id)
        .map(|t| format!("`{}` {}", t.id, t.status.state.label()))
        .collect();
    if !others.is_empty() {
        let _ = writeln!(out, "Other tasks in this conversation: {}", others.join(", "));
    }
    push_messages(&mut out, task);
    if let Some(result) = artifact_text(task, RESULT_ARTIFACT) {
        let _ = writeln!(out, "\nFinal answer:\n{}", clip(result, MAX_RESULT_CHARS));
    }
    push_evidence(&mut out, task, root).await;
    Some(out)
}

/// The latest task of every saved subagent conversation, newest first.
#[must_use]
pub(crate) fn render_list(contexts: &[SubagentContext]) -> String {
    let mut latest: Vec<(&Task, usize)> = contexts
        .iter()
        .filter_map(|c| c.latest_task().map(|t| (t, c.tasks.len())))
        .collect();
    if latest.is_empty() {
        return "No subagent tasks are saved in this session.".to_string();
    }
    latest.sort_by_key(|(task, _)| std::cmp::Reverse(task.status.timestamp));
    let mut out = String::from(
        "Subagent tasks in this session (latest task per conversation, newest first; inspect one with {\"task_id\": \"...\", \"inspect\": true}):\n",
    );
    for (task, count) in latest.iter().take(MAX_LISTED_CONTEXTS) {
        let problems = super::evidence::problem_count(&data::<Vec<ToolOutcome>>(task, TOOL_CALLS_ARTIFACT));
        let flag = if problems > 0 {
            format!(", {problems} failed tool calls")
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "  `{}` \u{b7} {}{flag} \u{b7} {} \u{b7} {count} task(s) \u{b7} \"{}\"",
            task.id,
            task.status.state.label(),
            settings_line(task),
            title(task)
        );
    }
    if latest.len() > MAX_LISTED_CONTEXTS {
        let _ = writeln!(out, "  ... {} older conversations", latest.len() - MAX_LISTED_CONTEXTS);
    }
    out
}
