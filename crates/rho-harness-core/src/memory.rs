//! Notes the model keeps across sessions with the memory tool: one markdown bullet per
//! note in `~/.agents/memory/global.md` and `~/.agents/memory/<project key>.md`.

use crate::args::MemoryScope;
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const MAX_NOTE_CHARS: usize = 300;
pub const MAX_NOTES: usize = 50;
pub const MAX_FILE_BYTES: usize = 8 * 1024;
const KEY_PATH_CHARS: usize = 80;
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const LOCK_RETRY: Duration = Duration::from_millis(10);

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("note text is empty")]
    Empty,
    #[error("a note must be a single line")]
    MultiLine,
    #[error("note is {0} characters; keep it under {MAX_NOTE_CHARS}")]
    TooLong(usize),
    #[error("note looks like a credential; never store secrets in memory")]
    LooksLikeSecret,
    #[error("note {0} already says this")]
    Duplicate(usize),
    #[error(
        "memory is full ({MAX_NOTES} notes or {MAX_FILE_BYTES} bytes per scope); view the notes and remove stale ones first"
    )]
    Full,
    #[error("no note {index}; there are {count} (view lists them)")]
    NoSuchNote { index: usize, count: usize },
    #[error("{}: another process is changing the notes; try again", path.display())]
    Locked { path: PathBuf },
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
}

/// The project and global note files under one memory directory.
#[derive(Debug, Clone)]
pub struct MemoryStore {
    dir: PathBuf,
    project_key: String,
    lock_timeout: Duration,
}

/// Notes read for the prompt, within the limits `add` enforces, and how many were left out.
#[derive(Debug, PartialEq, Eq)]
pub struct LoadedNotes {
    pub notes: Vec<String>,
    pub omitted: usize,
}

impl MemoryStore {
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>, workspace_root: &Path) -> Self {
        Self {
            dir: dir.into(),
            project_key: project_key(workspace_root),
            lock_timeout: LOCK_TIMEOUT,
        }
    }

    /// How long a change waits for another process's lock on the notes before giving up.
    #[must_use]
    pub fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout;
        self
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
        Ok(parse_notes(&self.raw(scope)?))
    }

    /// The notes a hand-edited file may hold more of than `add` allows: the first [`MAX_NOTES`]
    /// notes within [`MAX_FILE_BYTES`], each cut to [`MAX_NOTE_CHARS`] characters.
    pub fn loaded_notes(&self, scope: MemoryScope) -> Result<LoadedNotes, MemoryError> {
        let all = self.notes(scope)?;
        let mut notes = Vec::new();
        let mut bytes = 0;
        for note in &all {
            let note: String = note.chars().take(MAX_NOTE_CHARS).collect();
            bytes += note.len() + 3;
            if notes.len() == MAX_NOTES || bytes > MAX_FILE_BYTES {
                break;
            }
            notes.push(note);
        }
        Ok(LoadedNotes {
            omitted: all.len() - notes.len(),
            notes,
        })
    }

    fn raw(&self, scope: MemoryScope) -> Result<String, MemoryError> {
        let path = self.path(scope);
        match std::fs::read_to_string(&path) {
            Ok(content) => Ok(content),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            Err(source) => Err(MemoryError::Io { path, source }),
        }
    }

    /// Holds an exclusive lock on the scope's file while `change` reads and rewrites it, so two
    /// sessions never lose each other's notes.
    fn locked<T>(&self, scope: MemoryScope, change: impl FnOnce() -> Result<T, MemoryError>) -> Result<T, MemoryError> {
        let path = self.path(scope).with_extension("lock");
        let io_error = |source| MemoryError::Io {
            path: path.clone(),
            source,
        };
        std::fs::create_dir_all(&self.dir).map_err(io_error)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(io_error)?;
        let deadline = Instant::now() + self.lock_timeout;
        while let Err(error) = lock.try_lock() {
            match error {
                std::fs::TryLockError::WouldBlock if Instant::now() < deadline => std::thread::sleep(LOCK_RETRY),
                std::fs::TryLockError::WouldBlock => return Err(MemoryError::Locked { path }),
                std::fs::TryLockError::Error(source) => return Err(io_error(source)),
            }
        }
        change()
    }

    /// Saves `text` as the last note and returns its 1-based number. Lines in the file that are not
    /// notes are kept.
    pub fn add(&self, scope: MemoryScope, text: &str) -> Result<usize, MemoryError> {
        self.locked(scope, || {
            let mut content = self.raw(scope)?;
            let notes = parse_notes(&content);
            let note = checked_note(&notes, text)?;
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(&format!("- {note}\n"));
            if notes.len() + 1 > MAX_NOTES || content.len() > MAX_FILE_BYTES {
                return Err(MemoryError::Full);
            }
            self.save(scope, &content)?;
            Ok(notes.len() + 1)
        })
    }

    /// Deletes the note numbered `index` (1-based) and returns its text; other lines are kept.
    pub fn remove(&self, scope: MemoryScope, index: usize) -> Result<String, MemoryError> {
        self.locked(scope, || {
            let content = self.raw(scope)?;
            let count = parse_notes(&content).len();
            let mut seen = 0;
            let mut removed = None;
            let mut kept = String::with_capacity(content.len());
            for line in content.split_inclusive('\n') {
                if note_text(line).is_some() {
                    seen += 1;
                    if seen == index {
                        removed = note_text(line).map(str::to_string);
                        continue;
                    }
                }
                kept.push_str(line);
            }
            let removed = removed.ok_or(MemoryError::NoSuchNote { index, count })?;
            self.save(scope, &kept)?;
            Ok(removed)
        })
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

/// The note a bullet line holds (`- ` or `* `); `None` for any other line or an empty bullet.
fn note_text(line: &str) -> Option<&str> {
    let line = line.trim();
    line.strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .map(str::trim)
        .filter(|note| !note.is_empty())
}

/// Bullet lines (`- ` or `* `) are notes; any other line is not.
#[must_use]
pub fn parse_notes(content: &str) -> Vec<String> {
    content.lines().filter_map(note_text).map(str::to_string).collect()
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

const SECRET_KEYS: [&str; 9] = [
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "api-key",
    "apikey",
    "access_key",
    "private_key",
];

/// Credential shapes: provider key prefixes, `key=value` and `key: value` assignments, PEM private
/// key headers, bearer tokens, JWTs, URLs with a password, and long hex or base64 words.
fn looks_like_secret(note: &str) -> bool {
    let lower = note.to_ascii_lowercase();
    let alnum = |c: char| c.is_ascii_alphanumeric();
    let dashed = |c: char| alnum(c) || matches!(c, '-' | '_');
    assigns_secret(&lower)
        || (lower.contains("-----begin") && lower.contains("private key-----"))
        || has_token_after(note, "sk-", 20, dashed)
        || has_token_after(note, "ghp_", 20, alnum)
        || has_token_after(note, "github_pat_", 20, dashed)
        || has_token_after(note, "glpat-", 20, dashed)
        || has_token_after(note, "AKIA", 16, |c| c.is_ascii_uppercase() || c.is_ascii_digit())
        || has_token_after(note, "ASIA", 16, |c| c.is_ascii_uppercase() || c.is_ascii_digit())
        || has_token_after(note, "AIza", 35, dashed)
        || ["xoxb-", "xoxa-", "xoxp-", "xoxr-", "xoxs-"]
            .iter()
            .any(|prefix| has_token_after(note, prefix, 10, dashed))
        || has_token_after(&lower, "bearer ", 16, |c| {
            alnum(c) || matches!(c, '-' | '.' | '_' | '~' | '+' | '/' | '=')
        })
        || has_url_password(note)
        || note
            .split_whitespace()
            .any(|word| is_jwt(word) || is_long_encoded_word(word))
}

/// `password=`, `secret = x`, `token: x`: a credential name assigned a value. A bare `key:` with
/// nothing after it is prose, not an assignment.
fn assigns_secret(lower: &str) -> bool {
    SECRET_KEYS.iter().any(|key| {
        lower.match_indices(key).any(|(at, _)| {
            let rest = lower[at + key.len()..].trim_start_matches([' ', '\t']);
            rest.starts_with('=') || rest.strip_prefix(':').is_some_and(|value| !value.trim().is_empty())
        })
    })
}

/// `scheme://user:password@host`.
fn has_url_password(note: &str) -> bool {
    note.match_indices("://").any(|(at, _)| {
        let authority = note[at + 3..].split(['/', ' ', '\t']).next().unwrap_or("");
        authority
            .split_once('@')
            .and_then(|(userinfo, _)| userinfo.split_once(':'))
            .is_some_and(|(_, password)| !password.is_empty())
    })
}

fn is_jwt(word: &str) -> bool {
    let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    let parts: Vec<&str> = word.split('.').collect();
    word.starts_with("eyJ")
        && parts.len() == 3
        && parts.iter().all(|part| {
            part.len() >= 8
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        })
}

/// Whether `prefix` starts a word in `text` and is followed by at least `min` characters `accept`s.
fn has_token_after(text: &str, prefix: &str, min: usize, accept: impl Fn(char) -> bool) -> bool {
    text.match_indices(prefix).any(|(at, _)| {
        let starts_word = !text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric());
        starts_word && text[at + prefix.len()..].chars().take_while(|&c| accept(c)).count() >= min
    })
}

/// A SHA-1 is 40 hex digits and notes mention commits; any other long hex word is a key.
const GIT_SHA_LEN: usize = 40;

/// Long hex words, and long base64 words that look random: upper case makes up a quarter of the
/// word and it has digits, which CamelCase identifiers do not.
fn is_long_encoded_word(word: &str) -> bool {
    let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    if word.len() <= 32 {
        return false;
    }
    if word.chars().all(|c| c.is_ascii_hexdigit()) {
        return word.len() != GIT_SHA_LEN;
    }
    let count = |test: fn(&char) -> bool| word.chars().filter(&test).count();
    word.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_'))
        && count(char::is_ascii_uppercase) * 4 >= word.len()
        && count(char::is_ascii_lowercase) > 0
        && count(char::is_ascii_digit) > 0
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
    if looks_like_secret(note) {
        return Err(MemoryError::LooksLikeSecret);
    }
    match notes.iter().position(|existing| existing.eq_ignore_ascii_case(note)) {
        Some(i) => Err(MemoryError::Duplicate(i + 1)),
        None => Ok(note.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn store() -> (TempDir, MemoryStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path().join("memory"), &dir.path().join("workspace"));
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
        assert_eq!(
            store.path(MemoryScope::Global),
            dir.path().join("memory").join("global.md")
        );
    }

    #[test]
    fn invalid_notes_are_rejected_without_changing_the_file() {
        let (_dir, store) = store();
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
    }

    #[test]
    fn a_full_scope_asks_for_stale_notes_to_be_removed() {
        let (dir, store) = store();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
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
    }

    #[test]
    fn user_edited_files_keep_only_bullet_lines() {
        let content = "# Notes\n\n- first\n  * second  \nplain text\n-\n- third\n";
        assert_eq!(parse_notes(content), ["first", "second", "third"]);
        assert_eq!(numbered(&parse_notes(content)), "1. first\n2. second\n3. third\n");
    }

    #[test]
    fn project_keys_are_stable_readable_and_distinct() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("my repo");
        let b = dir.path().join("my-repo");
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
        assert!(project_key(&dir.path().join("x".repeat(200))).len() <= KEY_PATH_CHARS + 9);
        assert_eq!(project_key(Path::new("/")).len(), 8);
    }

    #[test]
    fn concurrent_adds_under_the_file_lock_lose_no_note() {
        let (_dir, store) = store();
        let store = Arc::new(store);
        let handles: Vec<_> = (0..2)
            .map(|thread| {
                let store = Arc::clone(&store);
                std::thread::spawn(move || {
                    for i in 0..10 {
                        store
                            .add(MemoryScope::Project, &format!("thread {thread} note {i}"))
                            .unwrap();
                    }
                })
            })
            .collect();
        handles.into_iter().for_each(|h| h.join().unwrap());
        let mut notes = store.notes(MemoryScope::Project).unwrap();
        notes.sort();
        let mut expected: Vec<String> = (0..2)
            .flat_map(|t| (0..10).map(move |i| format!("thread {t} note {i}")))
            .collect();
        expected.sort();
        assert_eq!(notes, expected);
    }

    #[test]
    fn non_bullet_lines_survive_add_and_remove() {
        let (_dir, store) = store();
        let path = store.path(MemoryScope::Project);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# My notes\n- first\nfree text\n- second\n").unwrap();
        store.add(MemoryScope::Project, "third").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# My notes\n- first\nfree text\n- second\n- third\n"
        );
        assert_eq!(store.remove(MemoryScope::Project, 1).unwrap(), "first");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# My notes\nfree text\n- second\n- third\n"
        );
    }

    #[test]
    fn a_file_without_a_trailing_newline_gets_one_before_the_new_note() {
        let (_dir, store) = store();
        let path = store.path(MemoryScope::Global);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "- first").unwrap();
        assert_eq!(store.add(MemoryScope::Global, "second").unwrap(), 2);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "- first\n- second\n");
    }

    #[test]
    fn an_unreadable_notes_path_is_an_io_error_naming_the_path() {
        let (_dir, store) = store();
        let path = store.path(MemoryScope::Project);
        std::fs::create_dir_all(&path).unwrap();
        for error in [
            store.notes(MemoryScope::Project).unwrap_err(),
            store.add(MemoryScope::Project, "x").unwrap_err(),
        ] {
            assert!(
                matches!(&error, MemoryError::Io { path: p, .. } if p == &path),
                "{error}"
            );
        }
    }

    #[test]
    fn notes_that_look_like_credentials_are_rejected() {
        let (_dir, store) = store();
        let hex = "a1b2c3d4".repeat(8);
        let b64 = "QWxhZGRpbjpvcGVuIHNlc2FtZVF3ZXJ0eTEyMzQ1Njc4";
        let b64url = "QWxhZGRpbjpvcGVuLXNlc2FtZV9Rd2VydHkxMjM0NTY3OA-_";
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r";
        let rejected = [
            "token is sk-abcdefghijklmnopqrstuvwx".to_string(),
            "use ghp_abcdefghijklmnopqrstuvwxyz0123456789".to_string(),
            "key AKIAIOSFODNN7EXAMPLE".to_string(),
            "temp key ASIAIOSFODNN7EXAMPLE".to_string(),
            "db PASSWORD=hunter2".to_string(),
            "password: hunter2".to_string(),
            "password = hunter2".to_string(),
            "the secret=".to_string(),
            "auth token=abc123".to_string(),
            "api_key: abc123".to_string(),
            "-----BEGIN RSA PRIVATE KEY-----".to_string(),
            "send Authorization: Bearer abcdef0123456789abcdef".to_string(),
            format!("hash {hex}"),
            format!("blob {b64}"),
            format!("blob {b64url}"),
            format!("jwt {jwt}"),
            "slack xoxb-123456789012-abcdefghijkl".to_string(),
            "pat github_pat_11ABCDEFG0abcdefghijklmnop_qrstuvwxyz".to_string(),
            "gitlab glpat-abcdefghij0123456789".to_string(),
            "maps AIzaSyA-abcdefghijklmnopqrstuvwxyz012345".to_string(),
            "clone https://deploy:s3cr3t@git.example.com/repo.git".to_string(),
        ];
        for note in rejected {
            let error = store.add(MemoryScope::Project, &note).unwrap_err();
            assert!(matches!(error, MemoryError::LooksLikeSecret), "{note}: {error}");
        }
        assert!(store.notes(MemoryScope::Project).unwrap().is_empty());
    }

    #[test]
    fn ordinary_notes_are_not_mistaken_for_credentials() {
        let (_dir, store) = store();
        let sha = "3b18e512dba79e4c8300dd08aeb37f8e728b8dad";
        for note in [
            "the task-force-orchestration-module is slow",
            "edit crates/rho-engine/src/tools/undo/tests for coverage",
            "reset the password in the settings page",
            "Bearer of bad news: CI is red",
            "set sk-1 to 2",
            "the regression came in with commit 3b18e512dba79e4c8300dd08aeb37f8e728b8dad",
            "see https://example.com/a:b@c and user@example.com",
            "HttpClientConfigurationFactoryBuilder2Impl owns the pool",
            "max_tokens=4096 is the default",
            "token: ",
        ] {
            store.add(MemoryScope::Project, note).unwrap();
        }
        assert_eq!(sha.len(), GIT_SHA_LEN);
    }

    #[test]
    fn hand_edited_files_load_within_the_limits_add_enforces() {
        let (_dir, store) = store();
        let path = store.path(MemoryScope::Project);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let many: Vec<String> = (0..80).map(|i| format!("note {i}")).collect();
        std::fs::write(&path, render_notes(&many)).unwrap();
        let loaded = store.loaded_notes(MemoryScope::Project).unwrap();
        assert_eq!((loaded.notes.len(), loaded.omitted), (MAX_NOTES, 30));

        let wide = vec!["w".repeat(MAX_NOTE_CHARS * 3); 40];
        std::fs::write(&path, render_notes(&wide)).unwrap();
        let loaded = store.loaded_notes(MemoryScope::Project).unwrap();
        let bytes: usize = loaded.notes.iter().map(|note| note.len() + 3).sum();
        assert!(
            bytes <= MAX_FILE_BYTES && loaded.omitted > 0,
            "{bytes} {}",
            loaded.omitted
        );
        assert!(loaded.notes.iter().all(|note| note.chars().count() == MAX_NOTE_CHARS));
    }

    #[test]
    fn a_note_file_locked_by_another_process_is_an_error_after_the_timeout() {
        let (_dir, store) = store();
        let store = store.with_lock_timeout(Duration::from_millis(30));
        store.add(MemoryScope::Project, "first").unwrap();
        let held = std::fs::File::open(store.path(MemoryScope::Project).with_extension("lock")).unwrap();
        held.lock().unwrap();

        let error = store.add(MemoryScope::Project, "second").unwrap_err();

        assert!(matches!(error, MemoryError::Locked { .. }), "{error}");
        held.unlock().unwrap();
        store.add(MemoryScope::Project, "second").unwrap();
    }
}
