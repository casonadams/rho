//! `rho stats`: tool-usage health from recorded sessions (call and error rates, recurring errors, and
//! file edits made through bash instead of the edit and write tools).

use rho_engine::permission::bash::{TokenKind, tokenize};
use rho_harness_core::session::{SessionEvent, SessionEventKind, SessionRecord};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const SCRATCH_PREFIXES: [&str; 5] = [
    "/dev/",
    "/tmp/",
    "/private/tmp/",
    "/var/folders/",
    "/private/var/folders/",
];
const SCRIPT_PROGRAMS: [&str; 6] = ["python", "python3", "node", "ruby", "perl", "php"];
const SCRIPT_WRITE_MARKERS: [&str; 8] = [
    "'w')",
    "\"w\")",
    "'a')",
    "\"a\")",
    ".write(",
    "write_text(",
    "writeFileSync",
    "File.write",
];
const TOP_ERRORS: usize = 5;
const EXAMPLES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BashEdit {
    SedInPlace,
    PerlInPlace,
    Script,
    Redirect,
    Tee,
}

impl BashEdit {
    fn label(self) -> &'static str {
        match self {
            Self::SedInPlace => "sed -i",
            Self::PerlInPlace => "perl -i",
            Self::Script => "script writes a file",
            Self::Redirect => "shell redirect into a file",
            Self::Tee => "tee into a file",
        }
    }
}

fn is_scratch(path: &str) -> bool {
    SCRATCH_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

/// The shell part of a command: everything after the first heredoc opener is input text, not shell.
fn shell_text(command: &str) -> &str {
    let mut end = 0;
    for line in command.split_inclusive('\n') {
        end += line.len();
        if line.contains("<<") {
            break;
        }
    }
    &command[..end]
}

fn segments(command: &str) -> Vec<Vec<rho_engine::permission::bash::Token>> {
    let mut out = vec![Vec::new()];
    for token in tokenize(shell_text(command)).tokens {
        if token.kind == TokenKind::Separator {
            out.push(Vec::new());
        } else if let Some(current) = out.last_mut() {
            current.push(token);
        }
    }
    out.retain(|segment| !segment.is_empty());
    out
}

fn segment_edit(segment: &[rho_engine::permission::bash::Token], command: &str) -> Option<BashEdit> {
    let words: Vec<&str> = segment
        .iter()
        .filter(|t| t.kind == TokenKind::Word)
        .map(|t| t.text.as_str())
        .collect();
    let program = words
        .first()
        .map(|w| w.rsplit('/').next().unwrap_or(w))
        .unwrap_or_default();
    let args = words.get(1..).unwrap_or_default();
    let has_flag = |short: char| {
        args.iter()
            .any(|a| a.starts_with('-') && !a.starts_with("--") && a[1..].contains(short))
    };
    match program {
        "sed" if has_flag('i') || args.contains(&"--in-place") => return Some(BashEdit::SedInPlace),
        "perl" if has_flag('i') => return Some(BashEdit::PerlInPlace),
        "tee" if args.iter().any(|a| !a.starts_with('-') && !is_scratch(a)) => return Some(BashEdit::Tee),
        p if SCRIPT_PROGRAMS.contains(&p) && SCRIPT_WRITE_MARKERS.iter().any(|m| command.contains(m)) => {
            return Some(BashEdit::Script);
        }
        _ => {}
    }
    let writes_workspace_file = segment.windows(2).any(|pair| {
        let (op, target) = (&pair[0], &pair[1]);
        op.kind == TokenKind::Redirect
            && op.raw.contains('>')
            && !op.raw.contains('<')
            && !op.raw.contains('&')
            && target.kind == TokenKind::Word
            && !target.text.starts_with('&')
            && !is_scratch(&target.text)
    });
    writes_workspace_file.then_some(BashEdit::Redirect)
}

/// How a bash command changed files without the edit or write tools, if it did.
pub fn classify_bash_edit(command: &str) -> Option<BashEdit> {
    segments(command)
        .iter()
        .find_map(|segment| segment_edit(segment, command))
}

/// Groups errors by their leading clause, dropping the specific path, value, or number that follows it.
fn error_signature(output: &str) -> String {
    let line = output.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
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
pub struct ToolCounts {
    pub calls: usize,
    pub errors: usize,
    pub error_signatures: BTreeMap<String, usize>,
}

#[derive(Debug, Default)]
pub struct ToolStats {
    pub sessions: usize,
    pub tools: BTreeMap<String, ToolCounts>,
    pub bash_edits: BTreeMap<BashEdit, usize>,
    pub bash_edit_examples: Vec<String>,
}

impl ToolStats {
    pub fn add_session(&mut self, events: &[SessionEvent]) {
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
            let signature = error_signature(payload["output"].as_str().unwrap_or_default());
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

    pub fn render(&self) -> String {
        let total: usize = self.tools.values().map(|c| c.calls).sum();
        let mut out = format!(
            "Tool usage across {} session(s), {total} tool call(s)\n\n",
            self.sessions
        );
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
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .filter_map(|line| serde_json::from_str::<SessionRecord>(line).ok())
        .filter_map(|record| match record {
            SessionRecord::AuditEvent { event, .. } => Some(event),
            _ => None,
        })
        .collect())
}

fn session_files(dir: &Path, last: Option<usize>) -> std::io::Result<Vec<PathBuf>> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|path| Some((std::fs::metadata(&path).ok()?.modified().ok()?, path)))
        .collect();
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    Ok(files
        .into_iter()
        .take(last.unwrap_or(usize::MAX))
        .map(|(_, path)| path)
        .collect())
}

pub fn collect_stats(sessions_dir: &Path, last: Option<usize>) -> std::io::Result<ToolStats> {
    let mut stats = ToolStats::default();
    if !sessions_dir.is_dir() {
        return Ok(stats);
    }
    for path in session_files(sessions_dir, last)?.iter().rev() {
        stats.add_session(&read_events(path)?);
    }
    Ok(stats)
}

pub fn handle_stats(last: Option<usize>, sessions_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    print!("{}", collect_stats(sessions_dir, last)?.render());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::session::SessionManager;
    use serde_json::json;

    #[test]
    fn bash_file_edits_are_classified_and_scratch_writes_ignored() {
        let cases = [
            ("sed -i '' 's/a/b/' src/lib.rs", Some(BashEdit::SedInPlace)),
            ("sed -E -i.bak 's/a/b/' x.rs", Some(BashEdit::SedInPlace)),
            ("sed -n 1,20p src/lib.rs", None),
            ("perl -pi -e 's/a/b/' x.rs", Some(BashEdit::PerlInPlace)),
            (
                "python3 - <<'EOF'\np='a.rs'\nopen(p,'w').write(s)\nEOF",
                Some(BashEdit::Script),
            ),
            ("python3 -c 'import json; print(json.load(open(\"a.json\")))'", None),
            ("cat > src/new.rs <<'EOF'\nfn main() {}\nEOF", Some(BashEdit::Redirect)),
            ("echo hi >> notes.md", Some(BashEdit::Redirect)),
            ("make all > /tmp/rho-make.log 2>&1", None),
            ("cargo test 2>&1 | grep FAIL", None),
            ("ls missing 2>/dev/null", None),
            ("echo x | tee out.txt", Some(BashEdit::Tee)),
            ("git status && cargo fmt --all", None),
            ("git commit -q -F - <<'EOF'\nfix: map Result<A> -> B > C\nEOF", None),
            ("git show HEAD:a.rs > a.rs", Some(BashEdit::Redirect)),
        ];
        for (command, expected) in cases {
            assert_eq!(classify_bash_edit(command), expected, "{command}");
        }
    }

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

    #[test]
    fn error_signatures_drop_specific_details() {
        let cases = [
            ("path not found: src/a.rs\nDid you mean: src/b.rs?", "path not found"),
            ("Command exited with code 101", "Command exited with code NNN"),
            (
                "Edit #2: oldText not found in file (exact match required):\nfoo",
                "Edit #N: oldText not found in file (exact match required):",
            ),
            (
                "Invalid arguments for edit: missing field `edits`. Pass ...",
                "Invalid arguments for edit",
            ),
            ("\n\n", ""),
        ];
        for (output, expected) in cases {
            assert_eq!(error_signature(output), expected, "{output}");
        }
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
        let dir = tempfile::tempdir().unwrap();
        for status in ["success", "error"] {
            let sm = SessionManager::new(dir.path(), None).unwrap();
            for e in tool("1", "rg", json!({ "pattern": "x" }), status, "path not found: x") {
                sm.append_event(e.kind, e.payload).await.unwrap();
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let all = collect_stats(dir.path(), None).unwrap();
        assert_eq!(all.sessions, 2);
        assert_eq!((all.tools["rg"].calls, all.tools["rg"].errors), (2, 1));

        let latest = collect_stats(dir.path(), Some(1)).unwrap();
        assert_eq!(latest.sessions, 1);
        assert_eq!(latest.tools["rg"].errors, 1, "the newest session is the failing one");

        let missing = collect_stats(&dir.path().join("none"), None).unwrap();
        assert_eq!(missing.sessions, 0);
    }
}
