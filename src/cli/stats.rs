//! `rho stats`: tool-usage health from recorded sessions (call and error rates, recurring errors, and
//! file edits made through bash instead of the edit and write tools).

use rho_engine::permission::bash::{Token, TokenKind, tokenize};
use rho_harness_core::session::{SessionEvent, SessionEventKind, SessionRecord};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const SCRATCH_PREFIXES: [&str; 7] = [
    "/dev/",
    "/tmp/",
    "/private/tmp/",
    "/var/folders/",
    "/private/var/folders/",
    "$TMPDIR/",
    "${TMPDIR}/",
];
const SCRIPT_PROGRAMS: [&str; 5] = ["python", "node", "ruby", "perl", "php"];
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
const WRAPPERS: [&str; 7] = ["env", "sudo", "command", "nohup", "time", "exec", "nice"];
const WRAPPER_VALUE_FLAGS: [&str; 5] = ["-u", "-g", "-C", "-n", "-S"];
const XARGS_VALUE_FLAGS: [&str; 8] = ["-I", "-n", "-P", "-L", "-s", "-d", "-E", "-a"];
const SED_VALUE_FLAGS: &str = "efl";
const PERL_VALUE_FLAGS: &str = "MmIeEdDxFC";
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

fn inside_quotes(prefix: &str) -> bool {
    prefix.matches('\'').count() % 2 == 1 || prefix.matches('"').count() % 2 == 1
}

fn heredoc_word(after_opener: &str) -> String {
    let after = after_opener.strip_prefix('-').unwrap_or(after_opener).trim_start();
    after
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, ';' | '&' | '|' | '<' | '>' | ')'))
        .filter(|c| !matches!(c, '\'' | '"' | '\\'))
        .collect()
}

fn heredoc_delimiters(line: &str) -> Vec<String> {
    line.match_indices("<<")
        .filter(|(pos, _)| !line[pos + 2..].starts_with('<') && !inside_quotes(&line[..*pos]))
        .map(|(pos, _)| heredoc_word(&line[pos + 2..]))
        .filter(|word| !word.is_empty())
        .collect()
}

struct ShellScript {
    text: String,
    heredoc_bodies: VecDeque<String>,
}

fn split_heredocs(command: &str) -> ShellScript {
    let mut script = ShellScript {
        text: String::new(),
        heredoc_bodies: VecDeque::new(),
    };
    let mut pending: VecDeque<String> = VecDeque::new();
    let mut body = String::new();
    for line in command.split_inclusive('\n') {
        let Some(delimiter) = pending.front() else {
            script.text.push_str(line);
            pending.extend(heredoc_delimiters(line));
            continue;
        };
        if line.trim() == delimiter {
            pending.pop_front();
            script.heredoc_bodies.push_back(std::mem::take(&mut body));
        } else {
            body.push_str(line);
        }
    }
    if !pending.is_empty() {
        script.heredoc_bodies.push_back(body);
    }
    script
}

#[derive(Default)]
struct Segment {
    words: Vec<String>,
    redirects: Vec<(String, Option<String>)>,
    heredoc: String,
}

struct SegmentBuilder {
    segments: Vec<Segment>,
    in_comment: bool,
    heredoc_bodies: VecDeque<String>,
}

impl SegmentBuilder {
    fn push(&mut self, token: Token, next_word: impl FnOnce() -> Option<String>) {
        let starts_line = token.kind == TokenKind::Separator && token.raw == "\n";
        if self.in_comment && !starts_line {
            return;
        }
        if token.kind == TokenKind::Separator {
            self.in_comment = false;
            self.segments.push(Segment::default());
            return;
        }
        let Some(current) = self.segments.last_mut() else {
            return;
        };
        match token.kind {
            TokenKind::Word if token.raw.starts_with('#') => self.in_comment = true,
            TokenKind::Word => current.words.push(token.text),
            _ => {
                if token.raw.starts_with("<<") {
                    current
                        .heredoc
                        .push_str(&self.heredoc_bodies.pop_front().unwrap_or_default());
                }
                current.redirects.push((token.raw, next_word()));
            }
        }
    }
}

fn segments(command: &str) -> Vec<Segment> {
    let script = split_heredocs(command);
    let mut builder = SegmentBuilder {
        segments: vec![Segment::default()],
        in_comment: false,
        heredoc_bodies: script.heredoc_bodies,
    };
    let mut tokens = tokenize(&script.text).tokens.into_iter().peekable();
    while let Some(token) = tokens.next() {
        let is_redirect = token.kind == TokenKind::Redirect;
        builder.push(token, || {
            is_redirect
                .then(|| tokens.next_if(|t| t.kind == TokenKind::Word))
                .flatten()
                .map(|t| t.text)
        });
    }
    builder.segments
}

fn program_name(word: &str) -> &str {
    let base = word.rsplit('/').next().unwrap_or(word);
    let versioned_python = base
        .strip_prefix("python")
        .is_some_and(|version| version.chars().all(|c| c.is_ascii_digit() || c == '.'));
    match base {
        "gsed" => "sed",
        _ if versioned_python => "python",
        _ => base,
    }
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn skip_options<'a, 'b>(words: &'a [&'b str], value_flags: &[&str]) -> &'a [&'b str] {
    let mut index = 0;
    while let Some(word) = words.get(index) {
        if is_assignment(word) {
            index += 1;
        } else if word.starts_with('-') {
            index += if value_flags.contains(word) { 2 } else { 1 };
        } else {
            break;
        }
    }
    &words[index.min(words.len())..]
}

fn effective_command<'a, 'b>(mut words: &'a [&'b str]) -> &'a [&'b str] {
    loop {
        let Some((first, rest)) = words.split_first() else {
            return words;
        };
        let program = program_name(first);
        words = if is_assignment(first) {
            rest
        } else if WRAPPERS.contains(&program) {
            skip_options(rest, &WRAPPER_VALUE_FLAGS)
        } else if program == "xargs" {
            skip_options(rest, &XARGS_VALUE_FLAGS)
        } else if let Some(exec) = (program == "find")
            .then(|| rest.iter().position(|w| matches!(*w, "-exec" | "-execdir" | "-ok")))
            .flatten()
        {
            &rest[exec + 1..]
        } else {
            return words;
        };
    }
}

/// Whether a short-option cluster such as `-ni` sets one of `flags` before an option that takes the rest as its value.
fn cluster_has(arg: &str, flags: &[char], value_flags: &str) -> bool {
    let Some(cluster) = arg.strip_prefix('-').filter(|c| !c.starts_with('-')) else {
        return false;
    };
    for c in cluster.chars() {
        if flags.contains(&c) {
            return true;
        }
        if value_flags.contains(c) {
            return false;
        }
    }
    false
}

fn only_scratch_paths(script: &str) -> bool {
    let quoted = |quote: char| script.split(quote).skip(1).step_by(2).collect::<Vec<_>>();
    let paths: Vec<&str> = [quoted('\''), quoted('"')]
        .concat()
        .into_iter()
        .filter(|literal| literal.contains('/'))
        .collect();
    !paths.is_empty() && paths.iter().all(|path| is_scratch(path))
}

fn script_writes_file(script: &str) -> bool {
    let script = script.replace("stdout.write(", "").replace("stderr.write(", "");
    SCRIPT_WRITE_MARKERS.iter().any(|m| script.contains(m)) && !only_scratch_paths(&script)
}

fn command_edit(command: &[&str], heredoc: &str) -> Option<BashEdit> {
    let (program, args) = command.split_first()?;
    let sed_in_place =
        |a: &&str| *a == "--in-place" || a.starts_with("--in-place=") || cluster_has(a, &['i', 'I'], SED_VALUE_FLAGS);
    match program_name(program) {
        "sed" if args.iter().any(sed_in_place) => Some(BashEdit::SedInPlace),
        "perl" if args.iter().any(|a| cluster_has(a, &['i'], PERL_VALUE_FLAGS)) => Some(BashEdit::PerlInPlace),
        "tee" if args.iter().any(|a| !a.starts_with('-') && !is_scratch(a)) => Some(BashEdit::Tee),
        p if SCRIPT_PROGRAMS.contains(&p) && script_writes_file(&format!("{}\n{heredoc}", args.join(" "))) => {
            Some(BashEdit::Script)
        }
        _ => None,
    }
}

fn writes_file(operator: &str) -> bool {
    let operator = operator.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
    operator.starts_with('>') && !operator.contains(['<', '&'])
}

fn is_file_target(target: &str) -> bool {
    !target.starts_with('&') && !target.contains(['(', ')']) && !is_scratch(target)
}

fn segment_edit(segment: &Segment) -> Option<BashEdit> {
    let words: Vec<&str> = segment.words.iter().map(String::as_str).collect();
    if let Some(edit) = command_edit(effective_command(&words), &segment.heredoc) {
        return Some(edit);
    }
    let is_test = words.first().is_some_and(|w| *w == "[[");
    let redirects_into_file = segment
        .redirects
        .iter()
        .any(|(op, target)| writes_file(op) && target.as_deref().is_some_and(is_file_target));
    (redirects_into_file && !is_test).then_some(BashEdit::Redirect)
}

/// How a bash command changed files without the edit or write tools, if it did.
pub fn classify_bash_edit(command: &str) -> Option<BashEdit> {
    segments(command).iter().find_map(segment_edit)
}

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
pub struct ToolCounts {
    pub calls: usize,
    pub errors: usize,
    pub error_signatures: BTreeMap<String, usize>,
}

#[derive(Debug, Default)]
pub struct ToolStats {
    pub sessions: usize,
    pub unreadable_sessions: usize,
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

    pub fn render(&self) -> String {
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
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|path| Some((std::fs::metadata(&path).ok()?.modified().ok()?, path)))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
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
        match read_events(path) {
            Ok(events) => stats.add_session(&events),
            Err(_) => stats.unreadable_sessions += 1,
        }
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
            ("rg -l foo | xargs sed -i 's/a/b/'", Some(BashEdit::SedInPlace)),
            ("rg -l foo | xargs -I {} sed -i 's/a/b/' {}", Some(BashEdit::SedInPlace)),
            (
                "find . -name '*.rs' -exec sed -i 's/a/b/' {} +",
                Some(BashEdit::SedInPlace),
            ),
            ("LC_ALL=C sed -i 's/a/b/' x.rs", Some(BashEdit::SedInPlace)),
            (
                "sudo -u me gsed --in-place=.bak 's/a/b/' x.rs",
                Some(BashEdit::SedInPlace),
            ),
            ("sed -I '' 's/a/b/' x.rs", Some(BashEdit::SedInPlace)),
            ("sed -e 's/i/x/' x.rs", None),
            (
                "env python3.12 -c \"open('a.rs','w').write('')\"",
                Some(BashEdit::Script),
            ),
            ("cargo build &> build.log", Some(BashEdit::Redirect)),
            ("cargo build &>> build.log", Some(BashEdit::Redirect)),
            ("perl -Mstrict -e 'print 1'", None),
            ("perl -MList::Util=min -Ilib -ne 'print' x", None),
            ("perl -pie 's/a/b/' x.rs", Some(BashEdit::PerlInPlace)),
            ("node -e \"process.stdout.write('x')\"", None),
            ("rg '\\.write\\(' src; python3 --version", None),
            ("python3 -c \"open('/tmp/x','w').write('')\"", None),
            ("echo $((1 > 0))", None),
            ("[[ $a > $b ]] && echo yes", None),
            ("cmd > >(tee /tmp/log)", None),
            ("echo hi # > x", None),
            ("echo $TMPDIR; echo x > $TMPDIR/out", None),
            (
                "cat > /tmp/p <<EOF\nbody > x\nEOF\nsed -i s/a/b/ src/x.rs",
                Some(BashEdit::SedInPlace),
            ),
            ("grep x <<< 'a > b'", None),
            ("git commit -m 'x << y'\nsed -i s/a/b/ f", Some(BashEdit::SedInPlace)),
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
            (
                "rg",
                "path not found: src/a.rs\nDid you mean: src/b.rs?",
                "path not found",
            ),
            ("bash", "Command exited with code 101", "Command exited with code NNN"),
            (
                "bash",
                "   Compiling foo v0.1.0\nerror[E0599]: oops\n\nCommand exited with code 101",
                "Command exited with code NNN",
            ),
            (
                "bash",
                "partial\n\nCommand timed out after 30 seconds",
                "Command timed out after NN seconds",
            ),
            (
                "edit",
                "Edit #2: oldText not found in file (exact match required):\nfoo",
                "Edit #N: oldText not found in file (exact match required):",
            ),
            (
                "edit",
                "Invalid arguments for edit: missing field `edits`. Pass ...",
                "Invalid arguments for edit",
            ),
            ("read", "\n\n", ""),
        ];
        for (tool, output, expected) in cases {
            assert_eq!(error_signature(tool, output), expected, "{output}");
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
        let base = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        for (age, status) in [(60, "success"), (0, "error")] {
            let sm = SessionManager::new(dir.path(), None).unwrap();
            for e in tool("1", "rg", json!({ "pattern": "x" }), status, "path not found: x") {
                sm.append_event(e.kind, e.payload).await.unwrap();
            }
            let file = std::fs::File::options().write(true).open(&sm.file_path).unwrap();
            file.set_modified(base - std::time::Duration::from_secs(age)).unwrap();
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

    #[test]
    fn unreadable_session_files_are_counted_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("broken.jsonl")).unwrap();
        std::fs::write(dir.path().join("torn.jsonl"), b"{\"type\":\xff\xfe").unwrap();

        let stats = collect_stats(dir.path(), None).unwrap();

        assert_eq!((stats.sessions, stats.unreadable_sessions), (1, 1));
        assert!(stats.render().contains("Skipped 1 unreadable session file(s)"));
    }
}
