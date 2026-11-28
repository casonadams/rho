use crate::permission::bash::{TokenKind, tokenize};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

const MAX_BASH_PATHS: usize = 50;

/// A file's content fingerprint; `None` when it does not exist or cannot be read.
pub(crate) type Fingerprint = Option<String>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WorkspaceChange {
    pub path: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
    /// The agent's call that was running when the change appeared, e.g. `bash: cargo fmt`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

impl WorkspaceChange {
    #[must_use]
    pub fn label(&self) -> String {
        let mut notes: Vec<String> = Vec::new();
        if self.deleted {
            notes.push("deleted".into());
        }
        notes.extend(self.by.clone());
        if notes.is_empty() {
            self.path.clone()
        } else {
            format!("{} ({})", self.path, notes.join("; "))
        }
    }
}

/// The directory tool paths are relative to: the project directory, else the process directory.
pub(crate) fn root_of(ctx: &crate::tools::engine_tool::TurnToolContext) -> PathBuf {
    ctx.project
        .as_ref()
        .map(|project| project.current_dir.clone())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default()
}

/// Blocking: hashes the whole file in chunks, so large files are compared by content too.
pub(crate) fn fingerprint(path: &Path) -> Fingerprint {
    let mut file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(hasher.finalize().iter().take(16).map(|b| format!("{b:02x}")).collect())
}

/// `raw` resolved against `root` with `.` and `..` removed lexically.
pub(crate) fn absolute(root: &Path, raw: &str) -> PathBuf {
    let joined = root.join(raw.trim());
    let mut out = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `path` relative to `root` when inside it, otherwise absolute, so one file always prints the same way.
pub(crate) fn display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(crate) fn normalize(root: &Path, raw: &str) -> String {
    display(root, &absolute(root, raw))
}

/// Fingerprints of `paths` (as displayed relative to `root`), computed off the async runtime.
pub(crate) async fn versions(root: &Path, paths: BTreeSet<String>) -> BTreeMap<String, Fingerprint> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|path| {
                let print = fingerprint(&absolute(&root, &path));
                (path, print)
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// The recorded paths whose content no longer matches their fingerprint, computed off the async runtime.
pub(crate) async fn stale_paths(root: &Path, recorded: &BTreeMap<String, Fingerprint>) -> Vec<String> {
    let root = root.to_path_buf();
    let recorded = recorded.clone();
    tokio::task::spawn_blocking(move || {
        recorded
            .into_iter()
            .filter(|(path, seen)| fingerprint(&absolute(&root, path)) != *seen)
            .map(|(path, _)| path)
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// Existing files named as arguments in a bash command (`cat src/a.rs`, `sed -n 1,9p b.rs`).
pub(crate) async fn bash_file_arguments(root: &Path, command: &str) -> Vec<String> {
    let words: Vec<String> = tokenize(command)
        .tokens
        .into_iter()
        .filter(|t| t.kind == TokenKind::Word && !t.text.starts_with('-') && !t.text.is_empty())
        .map(|t| t.text)
        .collect();
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let found: BTreeSet<String> = words
            .iter()
            .filter(|word| absolute(&root, word).is_file())
            .map(|word| normalize(&root, word))
            .take(MAX_BASH_PATHS)
            .collect();
        found.into_iter().collect()
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_normalize_to_one_spelling_relative_to_the_root() {
        let root = Path::new("/repo");
        let cases = [
            ("src/a.rs", "src/a.rs"),
            ("./src/a.rs", "src/a.rs"),
            ("/repo/src/a.rs", "src/a.rs"),
            ("src/../src/a.rs", "src/a.rs"),
            ("/elsewhere/b.rs", "/elsewhere/b.rs"),
        ];
        for (raw, expected) in cases {
            assert_eq!(normalize(root, raw), expected, "{raw}");
        }
    }

    #[test]
    fn labels_note_deletions_and_the_call_that_ran() {
        let change = |deleted, by: Option<&str>| WorkspaceChange {
            path: "a.rs".into(),
            deleted,
            by: by.map(str::to_string),
        };
        assert_eq!(change(false, None).label(), "a.rs");
        assert_eq!(
            change(true, Some("bash: rm a.rs")).label(),
            "a.rs (deleted; bash: rm a.rs)"
        );
    }

    #[test]
    fn large_files_are_fingerprinted_by_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.bin");
        let mut bytes = vec![7u8; 17 * 1024 * 1024];
        std::fs::write(&path, &bytes).unwrap();
        let first = fingerprint(&path);
        bytes[9 * 1024 * 1024] = 8;
        std::fs::write(&path, &bytes).unwrap();

        assert!(first.is_some());
        assert_ne!(fingerprint(&path), first);
        assert_eq!(fingerprint(dir.path()), None, "directories have no fingerprint");
    }

    #[tokio::test]
    async fn stale_paths_are_those_whose_content_changed_or_appeared() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("same.rs"), "a").unwrap();
        std::fs::write(dir.path().join("edited.rs"), "a").unwrap();
        let names: BTreeSet<String> = ["same.rs", "edited.rs", "later.rs"].map(String::from).into();
        let recorded = versions(dir.path(), names).await;

        std::fs::write(dir.path().join("edited.rs"), "b").unwrap();
        std::fs::write(dir.path().join("later.rs"), "new").unwrap();

        assert_eq!(stale_paths(dir.path(), &recorded).await, ["edited.rs", "later.rs"]);
    }

    #[tokio::test]
    async fn bash_arguments_that_are_existing_files_count_as_reads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/a.rs"), "").unwrap();
        std::fs::write(dir.path().join("b.rs"), "").unwrap();

        let read = bash_file_arguments(
            dir.path(),
            "cat ./src/a.rs && sed -n 1,9p b.rs | head -n 3 missing.rs src --color=b.rs",
        )
        .await;

        assert_eq!(read, ["b.rs", "src/a.rs"]);
    }
}
