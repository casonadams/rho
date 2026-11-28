//! Pre-run file snapshots for edit and write tool cards, shared by the main agent's terminal
//! renderer and the subagents' presenter so both cards read the same.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::locking::lock;
use rho_harness_core::args::EditReplacement;
use rho_harness_core::presentation::{FileChange, ToolLine};
use serde::Deserialize;
use serde_json::Value;
use std::ops::Range;

const MAX_SNAPSHOT_BYTES: u64 = 10 * 1024 * 1024;

/// The 1-based start line of an edit replacement in file content: where `old_text` is, or where
/// `new_text` is when the edit is already on disk.
pub(crate) fn locate_edit_line_number(content: &str, old_text: &str, new_text: &str) -> Option<usize> {
    locate_match_line(content, old_text).or_else(|| locate_match_line(content, new_text))
}

fn locate_match_line(content: &str, target: &str) -> Option<usize> {
    if target.trim().is_empty() {
        return None;
    }
    let line_of = |text: &str, idx: usize| 1 + text[..idx].matches('\n').count();
    if let Some(idx) = content.find(target) {
        return Some(line_of(content, idx));
    }
    if target.contains("\r\n") || content.contains("\r\n") {
        let norm_content = content.replace("\r\n", "\n");
        let norm_target = target.replace("\r\n", "\n");
        if let Some(idx) = norm_content.find(&norm_target) {
            return Some(line_of(&norm_content, idx));
        }
    }
    let first_line = target.lines().find(|l| !l.trim().is_empty())?;
    if content.matches(first_line).count() == 1 {
        return Some(line_of(content, content.find(first_line)?));
    }
    None
}

pub fn explicit_edit_line(edit: &Value) -> Option<usize> {
    edit.get("line")
        .or_else(|| edit.get("start_line"))
        .or_else(|| edit.get("line_number"))
        .and_then(Value::as_u64)
        .map(|line| line as usize)
}

pub fn read_bounded(path: &str) -> Option<String> {
    let too_large = std::fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_SNAPSHOT_BYTES);
    if too_large {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

pub fn edit_start_lines(edits: &[Value], content: Option<&str>) -> Vec<Option<usize>> {
    edits
        .iter()
        .map(|edit| {
            explicit_edit_line(edit).or_else(|| {
                let new = edit.get("newText").and_then(Value::as_str).unwrap_or("");
                locate_edit_line_number(content?, match_anchor(edit), new)
            })
        })
        .collect()
}

fn match_anchor(edit: &Value) -> &str {
    let text = |key: &str| edit.get(key).and_then(Value::as_str).filter(|s| !s.is_empty());
    text("oldText").or_else(|| text("startText")).unwrap_or("")
}

fn replaced_regions(edits: &[Value], content: Option<&str>) -> Vec<Option<String>> {
    let Some(content) = content else {
        return Vec::new();
    };
    let mut replay = EditReplay::new(content);
    for edit in edits {
        replay.apply(edit);
    }
    replay.regions
}

/// A file's content with edits applied in turn, and the region each `startText` edit replaced.
#[derive(Debug)]
struct EditReplay {
    current: String,
    line_ending: &'static str,
    regions: Vec<Option<String>>,
}

impl EditReplay {
    fn new(content: &str) -> Self {
        Self {
            current: content.to_string(),
            line_ending: super::edit::detect_line_ending(content),
            regions: Vec::new(),
        }
    }

    fn locate(&self, edit: &Value) -> Option<(EditReplacement, Vec<Range<usize>>)> {
        let parsed = EditReplacement::deserialize(edit).ok()?;
        let ranges = super::edit::matching::match_edit(&self.current, &parsed, self.line_ending).ok()?;
        Some((parsed, ranges))
    }

    fn region(&self, located: Option<&(EditReplacement, Vec<Range<usize>>)>) -> Option<String> {
        let (parsed, ranges) = located?;
        let range = ranges.first().filter(|_| parsed.start_text.is_some())?;
        Some(self.current[range.clone()].to_string())
    }

    /// The region `edit` would replace, without applying it.
    fn peek(&self, edit: &Value) -> Option<String> {
        self.region(self.locate(edit).as_ref())
    }

    fn apply(&mut self, edit: &Value) {
        let located = self.locate(edit);
        self.regions.push(self.region(located.as_ref()));
        if let Some((parsed, ranges)) = located {
            for range in ranges.into_iter().rev() {
                self.current.replace_range(range, &parsed.new_text);
            }
        }
    }
}

/// The file a `write` or `edit` call targets and its state before the call runs.
pub fn capture(name: &str, args: &Value) -> Option<(String, FileChange)> {
    let path = rho_harness_core::args::target_path(args)?;
    let change = match name {
        "write" => FileChange {
            overwrote_existing: std::path::Path::new(path).is_file(),
            edit_start_lines: Vec::new(),
            replaced_regions: Vec::new(),
        },
        "edit" => {
            let edits = args.get("edits")?.as_array()?;
            let content = read_bounded(path);
            FileChange {
                overwrote_existing: true,
                edit_start_lines: edit_start_lines(edits, content.as_deref()),
                replaced_regions: replaced_regions(edits, content.as_deref()),
            }
        }
        _ => return None,
    };
    Some((path.to_string(), change))
}

/// The file of an `edit` call whose arguments are still streaming, read once, with the edits
/// before the last one, which no longer change, already replayed on it.
#[derive(Debug)]
struct PreviewedFile {
    path: String,
    content: Option<Arc<str>>,
    settled: Vec<Value>,
    settled_lines: Vec<Option<usize>>,
    replay: Option<EditReplay>,
}

impl PreviewedFile {
    fn read(path: &str) -> Self {
        let content: Option<Arc<str>> = read_bounded(path).map(Arc::from);
        Self {
            path: path.to_string(),
            replay: content.as_deref().map(EditReplay::new),
            content,
            settled: Vec::new(),
            settled_lines: Vec::new(),
        }
    }

    fn change(&mut self, edits: &[Value]) -> FileChange {
        let (settled, last) = match edits.split_last() {
            Some((last, settled)) => (settled, Some(last)),
            None => (edits, None),
        };
        if !settled.starts_with(&self.settled) {
            self.settled.clear();
            self.settled_lines.clear();
            self.replay = self.content.as_deref().map(EditReplay::new);
        }
        let fresh = &settled[self.settled.len()..];
        self.settled_lines
            .extend(edit_start_lines(fresh, self.content.as_deref()));
        if let Some(replay) = &mut self.replay {
            fresh.iter().for_each(|edit| replay.apply(edit));
        }
        self.settled.extend_from_slice(fresh);

        let mut change = FileChange {
            overwrote_existing: true,
            edit_start_lines: self.settled_lines.clone(),
            replaced_regions: self.replay.as_ref().map(|r| r.regions.clone()).unwrap_or_default(),
        };
        if let Some(last) = last {
            let content = self.content.as_deref();
            change
                .edit_start_lines
                .extend(edit_start_lines(std::slice::from_ref(last), content));
            if let Some(replay) = &self.replay {
                change.replaced_regions.push(replay.peek(last));
            }
        }
        change
    }
}

/// Snapshots taken when tools start, handed to their finished lines.
#[derive(Debug, Default)]
pub struct PendingFileChanges {
    pending: Mutex<HashMap<String, FileChange>>,
    /// The file a call whose arguments are still streaming changes, read once for all its updates.
    previewed: Mutex<Option<PreviewedFile>>,
}

impl PendingFileChanges {
    /// Snapshots the file `name` is about to change, if it changes one, and returns the snapshot.
    pub fn note(&self, name: &str, args: &Value) -> Option<FileChange> {
        self.end_preview();
        self.record(capture(name, args)?)
    }

    /// Like `note`, for a call whose arguments are still streaming: its file is read once, however
    /// often the call is updated.
    pub fn note_preview(&self, name: &str, args: &Value) -> Option<FileChange> {
        if name != "edit" {
            return self.record(capture(name, args)?);
        }
        let path = rho_harness_core::args::target_path(args)?;
        let edits = args.get("edits")?.as_array()?;
        let change = {
            let mut previewed = lock(&self.previewed);
            let file = match previewed.take() {
                Some(file) if file.path == path => file,
                _ => PreviewedFile::read(path),
            };
            previewed.insert(file).change(edits)
        };
        self.record((path.to_string(), change))
    }

    /// Forgets the file read for a streaming call.
    pub fn end_preview(&self) {
        *lock(&self.previewed) = None;
    }

    fn record(&self, snapshot: (String, FileChange)) -> Option<FileChange> {
        let (path, change) = snapshot;
        lock(&self.pending).insert(path, change.clone());
        Some(change)
    }

    /// Gives `line` the snapshot taken for its file, unless it already has one.
    pub fn attach(&self, line: &mut ToolLine) {
        self.end_preview();
        if line.file_change.is_none()
            && let Some(path) = rho_harness_core::args::target_path(&line.arguments)
        {
            line.file_change = lock(&self.pending).remove(path);
        }
    }

    pub fn is_empty(&self) -> bool {
        lock(&self.pending).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use serde_json::json;

    #[test]
    fn a_finished_line_gets_the_state_its_file_had_when_the_tool_started() {
        let dir = TestDir::new();
        let file = dir.join("f.txt");
        std::fs::write(&file, "old").unwrap();
        let args = json!({"path": file.to_str().unwrap(), "content": "new"});
        let pending = PendingFileChanges::default();

        pending.note("write", &args);
        std::fs::write(&file, "new").unwrap();
        let mut line = ToolLine::new("write", args);
        pending.attach(&mut line);

        assert!(line.file_change.is_some_and(|change| change.overwrote_existing));
        assert!(pending.is_empty());
    }

    #[test]
    fn a_streaming_call_reads_its_file_once_and_its_start_reads_it_afresh() {
        let dir = TestDir::new();
        let file = dir.join("f.txt");
        std::fs::write(&file, "a\nb\n").unwrap();
        let args = json!({"path": file.to_str().unwrap(), "edits": [{"oldText": "b"}]});
        let lines = |change: Option<FileChange>| change.map(|change| change.edit_start_lines);
        let pending = PendingFileChanges::default();

        assert_eq!(lines(pending.note_preview("edit", &args)), Some(vec![Some(2)]));
        std::fs::write(&file, "x\na\nb\n").unwrap();
        assert_eq!(lines(pending.note_preview("edit", &args)), Some(vec![Some(2)]));
        assert_eq!(lines(pending.note("edit", &args)), Some(vec![Some(3)]));
    }

    #[test]
    fn a_streaming_edit_previews_what_a_fresh_capture_of_each_prefix_reads() {
        let dir = TestDir::new();
        let file = dir.join("f.rs");
        std::fs::write(&file, "fn a() {\n    one();\n}\nfn b() {\n    two();\n}\n").unwrap();
        let path = file.to_str().unwrap();
        let full = [
            json!({"oldText": "one();", "newText": "uno();\n    dos();"}),
            json!({"startText": "fn b() {", "endText": "}", "newText": "fn b() {}"}),
            json!({"oldText": "missing", "newText": "x"}),
            json!({"line": 2, "oldText": "dos", "newText": "tres"}),
        ];
        let pending = PendingFileChanges::default();
        for count in 0..=full.len() {
            for partial in [json!({"oldText": "fn "}), json!({"startText": "fn a"}), json!({})] {
                let mut edits = full[..count].to_vec();
                edits.push(partial);
                let args = json!({"path": path, "edits": edits});
                assert_eq!(
                    pending.note_preview("edit", &args),
                    capture("edit", &args).map(|(_, change)| change),
                    "{args}"
                );
            }
        }
    }

    #[test]
    fn tools_that_change_no_file_leave_no_snapshot() {
        let pending = PendingFileChanges::default();
        pending.note("rg", &json!({"path": "a.rs"}));
        let mut line = ToolLine::new("rg", json!({"path": "a.rs"}));
        pending.attach(&mut line);
        assert!(line.file_change.is_none() && pending.is_empty());
    }
}
