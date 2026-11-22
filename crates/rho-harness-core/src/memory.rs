//! Notes the model keeps across sessions with the memory tool: one markdown bullet per
//! note in `~/.agents/memory/global.md` and `~/.agents/memory/<project key>.md`.

use crate::args::MemoryScope;
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

pub const MAX_NOTE_CHARS: usize = 300;
pub const MAX_NOTES: usize = 50;
pub const MAX_FILE_BYTES: usize = 8 * 1024;
const KEY_PATH_CHARS: usize = 80;

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("note text is empty")]
    Empty,
    #[error("a note must be a single line")]
    MultiLine,
    #[error("note is {0} characters; keep it under {MAX_NOTE_CHARS}")]
    TooLong(usize),
    #[error("note {0} already says this")]
    Duplicate(usize),
    #[error(
        "memory is full ({MAX_NOTES} notes or {MAX_FILE_BYTES} bytes per scope); view the notes and remove stale ones first"
    )]
    Full,
    #[error("no note {index}; there are {count} (view lists them)")]
    NoSuchNote { index: usize, count: usize },
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
}

/// The project and global note files under one memory directory.
#[derive(Debug, Clone)]
pub struct MemoryStore {
    dir: PathBuf,
    project_key: String,
}

impl MemoryStore {
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>, workspace_root: &Path) -> Self {
        Self {
            dir: dir.into(),
            project_key: project_key(workspace_root),
        }
    }

    /// The store in `<home>/.agents/memory`.
    #[must_use]
    pub fn in_home(home: &Path, workspace_root: &Path) -> Self {
        Self::new(home.join(".agents").join("memory"), workspace_root)
    }

    #[must_use]
    pub fn path(&self, scope: MemoryScope) -> PathBuf {
        match scope {
            MemoryScope::Project => self.dir.join(format!("{}.md", self.project_key)),
            MemoryScope::Global => self.dir.join("global.md"),
        }
    }

    /// The saved notes in file order; a missing file has none.
    pub fn notes(&self, scope: MemoryScope) -> Result<Vec<String>, MemoryError> {
        let path = self.path(scope);
        match std::fs::read_to_string(&path) {
            Ok(content) => Ok(parse_notes(&content)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(source) => Err(MemoryError::Io { path, source }),
        }
    }

    /// Saves `text` as the last note and returns its 1-based number.
    pub fn add(&self, scope: MemoryScope, text: &str) -> Result<usize, MemoryError> {
        let mut notes = self.notes(scope)?;
        notes.push(checked_note(&notes, text)?);
        let content = render_notes(&notes);
        if notes.len() > MAX_NOTES || content.len() > MAX_FILE_BYTES {
            return Err(MemoryError::Full);
        }
        self.save(scope, &content)?;
        Ok(notes.len())
    }

    /// Deletes the note numbered `index` (1-based) and returns its text.
    pub fn remove(&self, scope: MemoryScope, index: usize) -> Result<String, MemoryError> {
        let mut notes = self.notes(scope)?;
        if index == 0 || index > notes.len() {
            return Err(MemoryError::NoSuchNote {
                index,
                count: notes.len(),
            });
        }
        let removed = notes.remove(index - 1);
        self.save(scope, &render_notes(&notes))?;
        Ok(removed)
    }

    fn save(&self, scope: MemoryScope, content: &str) -> Result<(), MemoryError> {
        let path = self.path(scope);
        crate::fs::write_atomic(&path, content.as_bytes(), 0o600).map_err(|source| MemoryError::Io { path, source })
    }
}

/// A readable, collision-safe file stem for a workspace: the sanitized tail of its canonical
/// path plus a hash of the whole path.
#[must_use]
pub fn project_key(workspace_root: &Path) -> String {
    let canonical = crate::workspace::canonicalize_path(workspace_root);
    let path = canonical.to_string_lossy();
    let mut readable = String::with_capacity(path.len());
    for c in path.chars() {
        let c = if c.is_ascii_alphanumeric() || matches!(c, '.' | '_') {
            c
        } else {
            '-'
        };
        if c != '-' || !readable.ends_with('-') {
            readable.push(c);
        }
    }
    let tail = &readable[readable.len().saturating_sub(KEY_PATH_CHARS)..];
    let tail = tail.trim_matches(['-', '.']);
    let hash = fnv1a(path.as_bytes()) >> 32;
    if tail.is_empty() {
        format!("{hash:08x}")
    } else {
        format!("{tail}-{hash:08x}")
    }
}

/// FNV-1a, 64-bit (offset basis and prime from the FNV specification).
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Bullet lines (`- ` or `* `) are notes; any other line is ignored.
#[must_use]
pub fn parse_notes(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            line.strip_prefix("- ").or_else(|| line.strip_prefix("* "))
        })
        .map(str::trim)
        .filter(|note| !note.is_empty())
        .map(str::to_string)
        .collect()
}

#[must_use]
pub fn render_notes(notes: &[String]) -> String {
    notes.iter().map(|note| format!("- {note}\n")).collect()
}

/// `1. first\n2. second\n`, the numbering remove takes.
#[must_use]
pub fn numbered(notes: &[String]) -> String {
    let mut out = String::new();
    for (i, note) in notes.iter().enumerate() {
        let _ = writeln!(out, "{}. {note}", i + 1);
    }
    out
}

fn checked_note(notes: &[String], text: &str) -> Result<String, MemoryError> {
    let note = text.trim();
    if note.is_empty() {
        return Err(MemoryError::Empty);
    }
    if note.contains(['\n', '\r']) {
        return Err(MemoryError::MultiLine);
    }
    let chars = note.chars().count();
    if chars > MAX_NOTE_CHARS {
        return Err(MemoryError::TooLong(chars));
    }
    match notes.iter().position(|existing| existing.eq_ignore_ascii_case(note)) {
        Some(i) => Err(MemoryError::Duplicate(i + 1)),
        None => Ok(note.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::created_temp_dir;

    fn store() -> (PathBuf, MemoryStore) {
        let dir = created_temp_dir();
        let store = MemoryStore::new(dir.join("memory"), &dir.join("workspace"));
        (dir, store)
    }

    #[test]
    fn notes_are_added_listed_and_removed_as_markdown_bullets() {
        let (dir, store) = store();
        assert!(store.notes(MemoryScope::Project).unwrap().is_empty());

        assert_eq!(
            store.add(MemoryScope::Project, "  fidelity tests are slow  ").unwrap(),
            1
        );
        assert_eq!(store.add(MemoryScope::Project, "src/vt is high risk").unwrap(), 2);
        assert_eq!(store.add(MemoryScope::Global, "prefer rg").unwrap(), 1);
        assert_eq!(
            std::fs::read_to_string(store.path(MemoryScope::Project)).unwrap(),
            "- fidelity tests are slow\n- src/vt is high risk\n"
        );

        assert_eq!(
            store.remove(MemoryScope::Project, 1).unwrap(),
            "fidelity tests are slow"
        );
        assert_eq!(store.notes(MemoryScope::Project).unwrap(), ["src/vt is high risk"]);
        assert_eq!(store.notes(MemoryScope::Global).unwrap(), ["prefer rg"]);
        assert_eq!(store.path(MemoryScope::Global), dir.join("memory").join("global.md"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_notes_are_rejected_without_changing_the_file() {
        let (dir, store) = store();
        store.add(MemoryScope::Project, "Run X first").unwrap();
        let long = "x".repeat(MAX_NOTE_CHARS + 1);
        let cases: [(&str, &str); 4] = [
            ("   ", "empty"),
            ("one\ntwo", "single line"),
            (&long, "301 characters"),
            ("run x FIRST", "note 1 already"),
        ];
        for (text, expected) in cases {
            let error = store.add(MemoryScope::Project, text).unwrap_err().to_string();
            assert!(error.contains(expected), "{text:?}: {error}");
        }
        let missing = store.remove(MemoryScope::Project, 2).unwrap_err().to_string();
        assert!(missing.contains("no note 2; there are 1"), "{missing}");
        assert!(store.remove(MemoryScope::Project, 0).is_err());
        assert_eq!(store.notes(MemoryScope::Project).unwrap(), ["Run X first"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_full_scope_asks_for_stale_notes_to_be_removed() {
        let (dir, store) = store();
        std::fs::create_dir_all(dir.join("memory")).unwrap();
        let many: Vec<String> = (1..=MAX_NOTES).map(|i| format!("note {i}")).collect();
        let big: Vec<String> = (0..27)
            .map(|i| format!("{i:02} {}", "y".repeat(MAX_NOTE_CHARS - 3)))
            .collect();
        assert!(render_notes(&big).len() <= MAX_FILE_BYTES);
        std::fs::write(store.path(MemoryScope::Global), render_notes(&many)).unwrap();
        std::fs::write(store.path(MemoryScope::Project), render_notes(&big)).unwrap();

        for scope in [MemoryScope::Global, MemoryScope::Project] {
            let full = store.add(scope, "one more note").unwrap_err().to_string();
            assert!(full.contains("remove stale ones"), "{full}");
        }
        assert_eq!(store.notes(MemoryScope::Global).unwrap(), many);
        assert_eq!(store.notes(MemoryScope::Project).unwrap(), big);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn user_edited_files_keep_only_bullet_lines() {
        let content = "# Notes\n\n- first\n  * second  \nplain text\n-\n- third\n";
        assert_eq!(parse_notes(content), ["first", "second", "third"]);
        assert_eq!(numbered(&parse_notes(content)), "1. first\n2. second\n3. third\n");
    }

    #[test]
    fn project_keys_are_stable_readable_and_distinct() {
        let dir = created_temp_dir();
        let a = dir.join("my repo");
        let b = dir.join("my-repo");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();

        let key = project_key(&a);
        assert_eq!(key, project_key(&a.join("sub").join("..")));
        assert!(key.contains("-my-repo-"), "{key}");
        assert!(
            key.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')),
            "{key}"
        );
        assert_ne!(key, project_key(&b));
        assert!(project_key(&dir.join("x".repeat(200))).len() <= KEY_PATH_CHARS + 9);
        assert_eq!(project_key(Path::new("/")).len(), 8);
        let _ = std::fs::remove_dir_all(dir);
    }
}
