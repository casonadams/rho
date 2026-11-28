//! `rho stats`: tool-usage health from recorded sessions (call and error rates, recurring errors, and
//! file edits made through bash instead of the edit and write tools).

use super::CliResult;
use rho_harness_core::session::{SessionEvent, SessionEventKind, SessionRecord};
use shell_edit::{BashEdit, classify_bash_edit};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

mod shell_edit;

const TOP_ERRORS: usize = 5;
const EXAMPLES: usize = 5;

/// The line that names a bash failure: rho's trailing exit or timeout line rather than the program's own output.
fn bash_failure_line(output: &str) -> &str {
    let last = output.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default();
    if last.starts_with("Command exited with code") || last.starts_with("Command timed out after") {
        last
    } else {
        output.lines().find(|l| !l.trim().is_empty()).unwrap_or_default()
    }
}

/// Groups errors by their leading clause, dropping the specific path, value, or number that follows it.
fn error_signature(tool: &str, output: &str) -> String {
    let line = if tool == "bash" {
        bash_failure_line(output)
    } else {
        output.lines().find(|l| !l.trim().is_empty()).unwrap_or_default()
    }
    .trim();
    let masked: String = line.chars().map(|c| if c.is_ascii_digit() { 'N' } else { c }).collect();
    let mut parts = masked.splitn(3, ": ");
    let head = parts.next().unwrap_or_default();
    let clause = match parts.next() {
        Some(detail) if head.starts_with("Edit #") => format!("{head}: {detail}"),
        _ => head.to_string(),
    };
    clause.chars().take(90).collect()
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ToolCounts {
    pub calls: usize,
    pub errors: usize,
    pub error_signatures: BTreeMap<String, usize>,
}

#[derive(Debug, Default)]
struct ToolStats {
    pub sessions: usize,
    pub unreadable_sessions: usize,
    pub tools: BTreeMap<String, ToolCounts>,
    pub bash_edits: BTreeMap<BashEdit, usize>,
    pub bash_edit_examples: Vec<String>,
}

impl ToolStats {
    fn add_session(&mut self, events: &[SessionEvent]) {
        self.sessions += 1;
        let mut commands: HashMap<&str, &str> = HashMap::new();
        for event in events {
            let payload = &event.payload;
            let id = payload["id"].as_str().unwrap_or_default();
            match event.kind {
                SessionEventKind::ToolCall => {
                    if let Some(command) = payload["arguments"]["command"].as_str() {
                        commands.insert(id, command);
                    }
                }
                SessionEventKind::ToolResult => self.add_result(payload, commands.get(id).copied()),
                _ => {}
            }
        }
    }

    fn add_result(&mut self, payload: &serde_json::Value, command: Option<&str>) {
        let name = payload["name"].as_str().unwrap_or("unknown").to_string();
        let counts = self.tools.entry(name.clone()).or_default();
        counts.calls += 1;
        if payload["status"].as_str() != Some("success") {
            counts.errors += 1;
            let signature = error_signature(&name, payload["output"].as_str().unwrap_or_default());
            *counts.error_signatures.entry(signature).or_default() += 1;
        }
        let Some(edit) = command.filter(|_| name == "bash").and_then(classify_bash_edit) else {
            return;
        };
        *self.bash_edits.entry(edit).or_default() += 1;
        if let Some(command) = command {
            self.bash_edit_examples
                .push(command.lines().next().unwrap_or_default().chars().take(100).collect());
        }
    }

    fn render(&self) -> String {
        let total: usize = self.tools.values().map(|c| c.calls).sum();
        let mut out = format!("Tool usage across {} session(s), {total} tool call(s)\n", self.sessions);
        if self.unreadable_sessions > 0 {
            let _ = writeln!(out, "Skipped {} unreadable session file(s)", self.unreadable_sessions);
        }
        out.push('\n');
        let _ = writeln!(out, "{:<16} {:>7} {:>7} {:>7}", "tool", "calls", "errors", "rate");
        let mut tools: Vec<(&String, &ToolCounts)> = self.tools.iter().collect();
        tools.sort_by(|a, b| b.1.calls.cmp(&a.1.calls).then_with(|| a.0.cmp(b.0)));
        for (name, counts) in &tools {
            let rate = 100.0 * counts.errors as f64 / counts.calls.max(1) as f64;
            let _ = writeln!(out, "{name:<16} {:>7} {:>7} {rate:>6.1}%", counts.calls, counts.errors);
        }
        self.render_errors(&mut out, &tools);
        self.render_bash_edits(&mut out);
        out
    }

    fn render_errors(&self, out: &mut String, tools: &[(&String, &ToolCounts)]) {
        let mut errors: Vec<(usize, &str, &str)> = tools
            .iter()
            .flat_map(|(name, counts)| {
                counts
                    .error_signatures
                    .iter()
                    .map(|(sig, n)| (*n, name.as_str(), sig.as_str()))
            })
            .collect();
        if errors.is_empty() {
            return;
        }
        errors.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)).then_with(|| a.2.cmp(b.2)));
        out.push_str("\nMost common errors:\n");
        for (count, name, signature) in errors.into_iter().take(TOP_ERRORS) {
            let _ = writeln!(out, "  {count:>4}x {name}: {signature}");
        }
    }

    fn render_bash_edits(&self, out: &mut String) {
        let bash_calls = self.tools.get("bash").map_or(0, |c| c.calls);
        let edits: usize = self.bash_edits.values().sum();
        let _ = writeln!(
            out,
            "\nFile edits through bash (should use edit/write): {edits} of {bash_calls} bash call(s)"
        );
        for (kind, count) in &self.bash_edits {
            let _ = writeln!(out, "  {count:>4}x {}", kind.label());
        }
        let start = self.bash_edit_examples.len().saturating_sub(EXAMPLES);
        if start < self.bash_edit_examples.len() {
            out.push_str("Most recent:\n");
            for example in &self.bash_edit_examples[start..] {
                let _ = writeln!(out, "  $ {example}");
            }
        }
    }
}

/// Audit events from one session file; lines that are not records of this format are skipped.
fn read_events(path: &Path) -> std::io::Result<Vec<SessionEvent>> {
    let bytes = std::fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| serde_json::from_str::<SessionRecord>(line).ok())
        .filter_map(|record| match record {
            SessionRecord::AuditEvent { event, .. } => Some(event),
            _ => None,
        })
        .collect())
}

fn session_files(dir: &Path, last: Option<usize>) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "jsonl") {
            files.push((std::fs::metadata(&path)?.modified()?, path));
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok(files
        .into_iter()
        .take(last.unwrap_or(usize::MAX))
        .map(|(_, path)| path)
        .collect())
}

fn collect_stats(sessions_dir: &Path, last: Option<usize>) -> std::io::Result<ToolStats> {
    let mut stats = ToolStats::default();
    if !sessions_dir.is_dir() {
        return Ok(stats);
    }
    for path in session_files(sessions_dir, last)?.iter().rev() {
        match read_events(path) {
            Ok(events) => stats.add_session(&events),
            Err(_) => stats.unreadable_sessions += 1,
        }
    }
    Ok(stats)
}

pub fn handle_stats(last: Option<usize>, sessions_dir: &Path) -> CliResult {
    print!("{}", collect_stats(sessions_dir, last)?.render());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::session::SessionManager;
    use serde_json::json;
    use test_case::test_case;

    fn event(kind: SessionEventKind, payload: serde_json::Value) -> SessionEvent {
        SessionEvent {
            id: "e".into(),
            timestamp: chrono::Utc::now(),
            kind,
            payload,
        }
    }

    fn tool(id: &str, name: &str, args: serde_json::Value, status: &str, output: &str) -> [SessionEvent; 2] {
        [
            event(
                SessionEventKind::ToolCall,
                json!({ "id": id, "name": name, "arguments": args }),
            ),
            event(
                SessionEventKind::ToolResult,
                json!({ "id": id, "name": name, "output": output, "status": status }),
            ),
        ]
    }

    #[test_case("rg", "path not found: src/a.rs\nDid you mean: src/b.rs?", "path not found"; "path suggestion")]
    #[test_case("bash", "Command exited with code 101", "Command exited with code NNN"; "exit code")]
    #[test_case(
        "bash",
        "   Compiling foo v0.1.0\nerror[E0599]: oops\n\nCommand exited with code 101",
        "Command exited with code NNN";
        "exit code after compiler output"
    )]
    #[test_case("bash", "partial\n\nCommand timed out after 30 seconds", "Command timed out after NN seconds"; "timeout")]
    #[test_case(
        "edit",
        "Edit #2: oldText not found in file (exact match required):\nfoo",
        "Edit #N: oldText not found in file (exact match required):";
        "edit number"
    )]
    #[test_case(
        "edit",
        "Invalid arguments for edit: missing field `edits`. Pass ...",
        "Invalid arguments for edit";
        "invalid arguments"
    )]
    #[test_case("read", "\n\n", ""; "blank output")]
    fn error_signatures_drop_specific_details(tool: &str, output: &str, expected: &str) {
        assert_eq!(error_signature(tool, output), expected);
    }

    #[test]
    fn stats_count_calls_errors_and_bash_edits() {
        let events: Vec<SessionEvent> = [
            tool("1", "read", json!({ "path": "a" }), "success", "ok"),
            tool(
                "2",
                "read",
                json!({ "path": "b" }),
                "error",
                "File not found: b (in 12 dirs)",
            ),
            tool(
                "3",
                "read",
                json!({ "path": "c" }),
                "error",
                "File not found: c (in 34 dirs)",
            ),
            tool(
                "4",
                "bash",
                json!({ "command": "sed -i '' 's/a/b/' x.rs" }),
                "success",
                "",
            ),
            tool("5", "bash", json!({ "command": "cargo test" }), "success", ""),
        ]
        .into_iter()
        .flatten()
        .collect();
        let mut stats = ToolStats::default();

        stats.add_session(&events);

        let read = &stats.tools["read"];
        assert_eq!((read.calls, read.errors), (3, 2));
        assert_eq!(stats.bash_edits[&BashEdit::SedInPlace], 1);
        let report = stats.render();
        assert!(
            report.contains("Tool usage across 1 session(s), 5 tool call(s)"),
            "{report}"
        );
        assert!(report.contains("read                   3       2   66.7%"), "{report}");
        assert!(report.contains("2x read: File not found\n"), "{report}");
        assert!(report.contains("1 of 2 bash call(s)"), "{report}");
        assert!(report.contains("$ sed -i '' 's/a/b/' x.rs"), "{report}");
    }

    #[tokio::test]
    async fn collects_from_recorded_session_files_and_limits_to_recent_sessions() {
        let dir = rho_test_support::TestDir::new();
        let base = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        for (age, status) in [(60, "success"), (0, "error")] {
            let sm = SessionManager::new(&dir, None).unwrap();
            for e in tool("1", "rg", json!({ "pattern": "x" }), status, "path not found: x") {
                sm.append_event(e.kind, e.payload).await.unwrap();
            }
            let file = std::fs::File::options().write(true).open(&sm.file_path).unwrap();
            file.set_modified(base - std::time::Duration::from_secs(age)).unwrap();
        }

        let all = collect_stats(&dir, None).unwrap();
        assert_eq!(all.sessions, 2);
        assert_eq!((all.tools["rg"].calls, all.tools["rg"].errors), (2, 1));

        let latest = collect_stats(&dir, Some(1)).unwrap();
        assert_eq!(latest.sessions, 1);
        assert_eq!(latest.tools["rg"].errors, 1, "the newest session is the failing one");

        let missing = collect_stats(&dir.join("none"), None).unwrap();
        assert_eq!(missing.sessions, 0);
    }

    #[test]
    fn unreadable_session_files_are_counted_not_fatal() {
        let dir = rho_test_support::TestDir::new();
        std::fs::create_dir(dir.join("broken.jsonl")).unwrap();
        std::fs::write(dir.join("torn.jsonl"), b"{\"type\":\xff\xfe").unwrap();

        let stats = collect_stats(&dir, None).unwrap();

        assert_eq!((stats.sessions, stats.unreadable_sessions), (1, 1));
        assert!(stats.render().contains("Skipped 1 unreadable session file(s)"));
    }
}
