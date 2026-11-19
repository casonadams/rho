use crate::tools::subagent::runner::Round;
use crate::tools::subagent::workspace::WorkspaceChange;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::ops::Range;
use std::time::Instant;

struct AgentFiles {
    step: String,
    changed: BTreeSet<String>,
    workspace: BTreeSet<WorkspaceChange>,
    ran: Range<Instant>,
}

/// Which workflow agents changed which files, and when they ran.
#[derive(Default)]
pub(crate) struct AgentFileLog {
    agents: Vec<AgentFiles>,
}

fn overlapping(a: &Range<Instant>, b: &Range<Instant>) -> bool {
    a.start < b.end && b.start < a.end
}

impl AgentFileLog {
    pub(crate) fn add(&mut self, step: &str, round: &Round, ran: Range<Instant>) {
        if round.changed_files.is_empty() && round.workspace_changes.is_empty() {
            return;
        }
        let own_bash = round
            .workspace_changes
            .iter()
            .filter(|c| c.by.is_some())
            .map(|c| c.path.clone());
        self.agents.push(AgentFiles {
            step: step.to_string(),
            changed: round.changed_files.iter().cloned().chain(own_bash).collect(),
            workspace: round.workspace_changes.clone(),
            ran,
        });
    }

    /// Files changed by agents that ran at the same time, with the agents involved.
    fn concurrent_edits(&self) -> BTreeMap<&str, BTreeSet<&str>> {
        let mut clashes: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for (i, a) in self.agents.iter().enumerate() {
            for b in &self.agents[i + 1..] {
                if !overlapping(&a.ran, &b.ran) {
                    continue;
                }
                for file in a.changed.intersection(&b.changed) {
                    clashes
                        .entry(file)
                        .or_default()
                        .extend([a.step.as_str(), b.step.as_str()]);
                }
            }
        }
        clashes
    }

    pub(crate) fn section(&self) -> String {
        let mut by_file: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for agent in &self.agents {
            for file in &agent.changed {
                by_file.entry(file).or_default().push(&agent.step);
            }
        }
        let others: BTreeSet<String> = self
            .agents
            .iter()
            .flat_map(|a| &a.workspace)
            .filter(|change| !by_file.contains_key(change.path.as_str()))
            .map(WorkspaceChange::label)
            .collect();
        let mut out = String::new();
        if !by_file.is_empty() {
            out.push_str("\nFiles changed by agents:\n");
            for (file, steps) in &by_file {
                let _ = writeln!(out, "  {file} \u{2014} {}", steps.join(", "));
            }
        }
        if !others.is_empty() {
            let list: Vec<String> = others.into_iter().collect();
            let _ = writeln!(
                out,
                "Also changed in the workspace while agents ran (bash, formatters, or other processes): {}",
                list.join(", ")
            );
        }
        for (file, steps) in self.concurrent_edits() {
            let steps: Vec<&str> = steps.into_iter().collect();
            let _ = writeln!(
                out,
                "\nWarning: {file} was changed by agents that ran at the same time ({}); check that one did not overwrite the other.",
                steps.join(", ")
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::subagent::runner::RoundEnd;
    use std::time::Duration;

    fn round(changed: &[&str], workspace: &[(&str, Option<&str>)]) -> Round {
        Round {
            end: RoundEnd::Answered("ok".into()),
            transcript: Vec::new(),
            turns: 1,
            tool_calls: 0,
            tokens: 0,
            changed_files: changed.iter().map(|f| (*f).to_string()).collect(),
            workspace_changes: workspace
                .iter()
                .map(|(f, by)| WorkspaceChange {
                    path: (*f).to_string(),
                    deleted: false,
                    by: by.map(str::to_string),
                })
                .collect(),
            workspace_complete: true,
            workspace_after: None,
            tool_log: Vec::new(),
            touched_files: BTreeSet::new(),
        }
    }

    #[test]
    fn lists_files_per_agent_and_warns_only_about_concurrent_edits() {
        let t0 = Instant::now();
        let at = |from: u64, to: u64| t0 + Duration::from_secs(from)..t0 + Duration::from_secs(to);
        let mut log = AgentFileLog::default();
        log.add(
            "scan #1",
            &round(&["a.rs", "shared.rs"], &[("a.rs", None), ("notes.md", None)]),
            at(0, 10),
        );
        log.add("scan #2", &round(&[], &[("shared.rs", Some("sed -i"))]), at(5, 15));
        log.add("later", &round(&["a.rs"], &[]), at(20, 30));
        log.add("idle", &round(&[], &[]), at(0, 30));

        assert_eq!(
            log.section(),
            "\nFiles changed by agents:\n  a.rs \u{2014} scan #1, later\n  shared.rs \u{2014} scan #1, scan #2\n\
             Also changed in the workspace while agents ran (bash, formatters, or other processes): notes.md\n\
             \nWarning: shared.rs was changed by agents that ran at the same time (scan #1, scan #2); check that one did not overwrite the other.\n"
        );
        assert_eq!(AgentFileLog::default().section(), "");
    }
}
