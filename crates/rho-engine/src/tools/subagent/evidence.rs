use super::workspace::WorkspaceChange;
use rho_harness_core::text::truncate_chars;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

const MAX_TARGET_CHARS: usize = 80;
const MAX_DETAIL_CHARS: usize = 160;
const MAX_LISTED_PROBLEMS: usize = 5;
const TARGET_FIELDS: [&str; 6] = ["path", "command", "pattern", "url", "query", "prompt"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeStatus {
    Ok,
    Error,
    Denied,
}

impl OutcomeStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Denied => "denied",
        }
    }
}

/// One tool call a subagent made and how it ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ToolOutcome {
    pub tool: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
    pub status: OutcomeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub(crate) fn clip(text: &str, max: usize) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    match truncate_chars(line, max) {
        (kept, true) => format!("{kept}..."),
        (_, false) => line.to_string(),
    }
}

impl ToolOutcome {
    #[must_use]
    pub fn new(tool: &str, args: &Value, status: OutcomeStatus, output: &str) -> Self {
        let target = TARGET_FIELDS
            .iter()
            .find_map(|field| args[*field].as_str())
            .map(|value| clip(value, MAX_TARGET_CHARS))
            .unwrap_or_default();
        Self {
            tool: tool.to_string(),
            target,
            status,
            detail: (status != OutcomeStatus::Ok)
                .then(|| clip(output, MAX_DETAIL_CHARS))
                .filter(|d| !d.is_empty()),
        }
    }

    #[must_use]
    pub fn call(&self) -> String {
        if self.target.is_empty() {
            self.tool.clone()
        } else {
            format!("{} {}", self.tool, self.target)
        }
    }

    #[must_use]
    pub fn line(&self) -> String {
        match &self.detail {
            Some(detail) => format!("{:<7} {} \u{2014} {detail}", self.status.label(), self.call()),
            None => format!("{:<7} {}", self.status.label(), self.call()),
        }
    }
}

/// Tool calls that errored or were denied.
#[must_use]
pub(crate) fn problem_count(log: &[ToolOutcome]) -> usize {
    log.iter().filter(|o| o.status != OutcomeStatus::Ok).count()
}

/// `"1 failed tool call"` or `"N failed tool calls"`, or `None` when nothing failed.
pub(crate) fn failed_calls_text(problems: usize) -> Option<String> {
    match problems {
        0 => None,
        1 => Some("1 failed tool call".to_string()),
        n => Some(format!("{n} failed tool calls")),
    }
}

fn count(log: &[ToolOutcome], status: OutcomeStatus) -> usize {
    log.iter().filter(|o| o.status == status).count()
}

/// `Tool outcomes: 12 ok, 2 errors (edit a.rs: oldText not found; ...), 1 denied (bash rm x)`.
#[must_use]
pub(crate) fn outcome_line(log: &[ToolOutcome]) -> Option<String> {
    if log.is_empty() {
        return None;
    }
    let mut parts = vec![format!("{} ok", count(log, OutcomeStatus::Ok))];
    for (status, noun) in [(OutcomeStatus::Error, "error"), (OutcomeStatus::Denied, "denied")] {
        let problems: Vec<&ToolOutcome> = log.iter().filter(|o| o.status == status).collect();
        if problems.is_empty() {
            continue;
        }
        let mut listed: Vec<String> = problems
            .iter()
            .take(MAX_LISTED_PROBLEMS)
            .map(|o| match &o.detail {
                Some(detail) => format!("{}: {detail}", o.call()),
                None => o.call(),
            })
            .collect();
        if problems.len() > MAX_LISTED_PROBLEMS {
            listed.push(format!("{} more", problems.len() - MAX_LISTED_PROBLEMS));
        }
        let plural = if noun == "error" && problems.len() != 1 {
            "s"
        } else {
            ""
        };
        parts.push(format!("{} {noun}{plural} ({})", problems.len(), listed.join("; ")));
    }
    Some(format!("Tool outcomes: {}", parts.join(", ")))
}

/// The evidence lines appended to a subagent answer: files it changed, other workspace changes, and tool outcomes.
#[must_use]
pub fn evidence(
    changed: &BTreeSet<String>,
    workspace: &BTreeSet<WorkspaceChange>,
    workspace_complete: bool,
    log: &[ToolOutcome],
) -> String {
    let mut lines = Vec::new();
    if !changed.is_empty() {
        let files: Vec<&str> = changed.iter().map(String::as_str).collect();
        lines.push(format!("Files changed: {}", files.join(", ")));
    }
    let (by_calls, others): (Vec<&WorkspaceChange>, Vec<&WorkspaceChange>) = workspace
        .iter()
        .filter(|c| !changed.contains(&c.path))
        .partition(|c| c.by.is_some());
    let labels = |changes: &[&WorkspaceChange]| changes.iter().map(|c| c.label()).collect::<Vec<_>>().join(", ");
    if !by_calls.is_empty() {
        lines.push(format!("Changed while its commands ran: {}", labels(&by_calls)));
    }
    if !others.is_empty() {
        lines.push(format!(
            "Also changed in the workspace during this run, not by its tools (editors, other agents, or processes): {}",
            labels(&others)
        ));
    }
    if !workspace_complete {
        lines.push(
            "Workspace check incomplete: too many files to compare, so changes made outside write/edit may be missing."
                .to_string(),
        );
    }
    lines.extend(outcome_line(log));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use test_case::test_case;

    fn outcome(tool: &str, args: Value, status: OutcomeStatus, output: &str) -> ToolOutcome {
        ToolOutcome::new(tool, &args, status, output)
    }

    #[test_case("read", json!({ "path": "a.rs" }), OutcomeStatus::Ok, "body", "ok      read a.rs"; "ok read")]
    #[test_case(
        "edit",
        json!({ "path": "b.rs" }),
        OutcomeStatus::Error,
        "\noldText not found\nmore",
        "error   edit b.rs \u{2014} oldText not found";
        "edit error"
    )]
    #[test_case(
        "bash",
        json!({ "command": "rm -rf x\necho" }),
        OutcomeStatus::Denied,
        "Denied by rule",
        "denied  bash rm -rf x \u{2014} Denied by rule";
        "denied bash"
    )]
    #[test_case("web_search", json!({}), OutcomeStatus::Ok, "", "ok      web_search"; "no target")]
    fn outcomes_name_their_target_and_first_error_line(
        tool: &str,
        args: Value,
        status: OutcomeStatus,
        output: &str,
        expected: &str,
    ) {
        assert_eq!(outcome(tool, args, status, output).line(), expected);
    }

    #[test]
    fn outcome_line_counts_and_lists_problems() {
        assert_eq!(outcome_line(&[]), None);
        let mut log = vec![outcome("read", json!({ "path": "a.rs" }), OutcomeStatus::Ok, "")];
        assert_eq!(outcome_line(&log).unwrap(), "Tool outcomes: 1 ok");
        for i in 0..7 {
            log.push(outcome(
                "edit",
                json!({ "path": format!("f{i}.rs") }),
                OutcomeStatus::Error,
                "nope",
            ));
        }
        log.push(outcome("bash", json!({ "command": "rm x" }), OutcomeStatus::Denied, ""));

        let line = outcome_line(&log).unwrap();

        assert!(
            line.starts_with("Tool outcomes: 1 ok, 7 errors (edit f0.rs: nope; "),
            "{line}"
        );
        assert!(
            line.contains("edit f4.rs: nope; 2 more), 1 denied (bash rm x)"),
            "{line}"
        );
        assert_eq!(problem_count(&log), 8);
    }

    #[test]
    fn evidence_separates_agent_edits_from_other_workspace_changes() {
        let changed = BTreeSet::from(["a.rs".to_string()]);
        let change = |path: &str, deleted, by: Option<&str>| WorkspaceChange {
            path: path.into(),
            deleted,
            by: by.map(str::to_string),
        };
        let workspace = BTreeSet::from([
            change("a.rs", false, None),
            change("fmt.rs", false, Some("bash: cargo fmt")),
            change("notes.md", false, None),
            change("old.rs", true, None),
        ]);
        assert_eq!(
            evidence(&changed, &workspace, true, &[]),
            "Files changed: a.rs\nChanged while its commands ran: fmt.rs (bash: cargo fmt)\n\
             Also changed in the workspace during this run, not by its tools (editors, other agents, or processes): notes.md, old.rs (deleted)"
        );
        assert_eq!(evidence(&BTreeSet::new(), &BTreeSet::new(), true, &[]), "");
        assert!(evidence(&BTreeSet::new(), &BTreeSet::new(), false, &[]).starts_with("Workspace check incomplete"));
    }
}
