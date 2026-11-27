//! Pre-run file snapshots for edit and write tool cards, shared by the main agent's terminal
//! renderer and the subagents' presenter so both cards read the same.

use std::collections::HashMap;
use std::sync::Mutex;

use rho_harness_core::presentation::{FileChange, ToolLine};
use serde_json::Value;

const MAX_SNAPSHOT_BYTES: u64 = 10 * 1024 * 1024;

/// The 1-based start line of an edit replacement in file content: where `old_text` is, or where
/// `new_text` is when the edit is already on disk.
pub fn locate_edit_line_number(content: &str, old_text: &str, new_text: &str) -> Option<usize> {
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
    let Some(mut current) = content.map(str::to_string) else {
        return Vec::new();
    };
    let line_ending = super::edit::detect_line_ending(&current);
    let mut regions = Vec::with_capacity(edits.len());
    for edit in edits {
        let parsed = serde_json::from_value::<rho_harness_core::args::EditReplacement>(edit.clone()).ok();
        let located = parsed.as_ref().and_then(|e| {
            let ranges = super::edit::locate_edit(&current, e, line_ending).ok()?;
            Some((e, ranges))
        });
        let Some((parsed, ranges)) = located else {
            regions.push(None);
            continue;
        };
        let region = ranges
            .first()
            .filter(|_| parsed.start_text.is_some())
            .map(|r| current[r.clone()].to_string());
        regions.push(region);
        for range in ranges.into_iter().rev() {
            current.replace_range(range, &parsed.new_text);
        }
    }
    regions
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

/// Snapshots taken when tools start, handed to their finished lines.
#[derive(Debug, Default)]
pub struct PendingFileChanges(Mutex<HashMap<String, FileChange>>);

impl PendingFileChanges {
    /// Snapshots the file `name` is about to change, if it changes one.
    pub fn note(&self, name: &str, args: &Value) {
        if let Some((path, change)) = capture(name, args)
            && let Ok(mut pending) = self.0.lock()
        {
            pending.insert(path, change);
        }
    }

    /// Gives `line` the snapshot taken for its file, unless it already has one.
    pub fn attach(&self, line: &mut ToolLine) {
        if line.file_change.is_none()
            && let Some(path) = rho_harness_core::args::target_path(&line.arguments)
            && let Ok(mut pending) = self.0.lock()
        {
            line.file_change = pending.remove(path);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.lock().is_ok_and(|pending| pending.is_empty())
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
    fn tools_that_change_no_file_leave_no_snapshot() {
        let pending = PendingFileChanges::default();
        pending.note("rg", &json!({"path": "a.rs"}));
        let mut line = ToolLine::new("rg", json!({"path": "a.rs"}));
        pending.attach(&mut line);
        assert!(line.file_change.is_none() && pending.is_empty());
    }
}
