use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

const MAX_HASHED_BYTES: u64 = 16 * 1024 * 1024;

/// A file's content fingerprint; `None` when it does not exist or cannot be read.
pub(crate) type Fingerprint = Option<String>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WorkspaceChange {
    pub path: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

impl WorkspaceChange {
    #[must_use]
    pub fn label(&self) -> String {
        if self.deleted {
            format!("{} (deleted)", self.path)
        } else {
            self.path.clone()
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

pub(crate) fn fingerprint(path: &Path) -> Fingerprint {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    if meta.len() > MAX_HASHED_BYTES {
        let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
        return Some(format!("size:{}:mtime:{}", meta.len(), modified.as_nanos()));
    }
    let digest = Sha256::digest(std::fs::read(path).ok()?);
    Some(digest.iter().take(16).map(|b| format!("{b:02x}")).collect())
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

/// Fingerprints of `paths` (as displayed relative to `root`).
pub(crate) fn versions<'a>(root: &Path, paths: impl IntoIterator<Item = &'a String>) -> BTreeMap<String, Fingerprint> {
    paths
        .into_iter()
        .map(|path| (path.clone(), fingerprint(&absolute(root, path))))
        .collect()
}

/// The recorded paths whose content no longer matches their fingerprint.
pub(crate) fn stale_paths(root: &Path, recorded: &BTreeMap<String, Fingerprint>) -> Vec<String> {
    recorded
        .iter()
        .filter(|(path, seen)| fingerprint(&absolute(root, path)) != **seen)
        .map(|(path, _)| path.clone())
        .collect()
}

/// Uncommitted files in the git work tree and their content, used to see what a round changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DirtySnapshot {
    files: BTreeMap<PathBuf, Fingerprint>,
}

async fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    output.status.success().then_some(output.stdout)
}

/// Paths named by `git status --porcelain=v1 -z`; renames and copies name both the new and the original path.
fn porcelain_paths(stdout: &[u8]) -> Vec<String> {
    let mut paths = Vec::new();
    let mut entries = stdout.split(|b| *b == 0).filter(|e| e.len() > 3);
    while let Some(entry) = entries.next() {
        let text = String::from_utf8_lossy(entry);
        paths.push(text[3..].to_string());
        if text[..2].contains(['R', 'C']) {
            paths.extend(entries.next().map(|orig| String::from_utf8_lossy(orig).into_owned()));
        }
    }
    paths
}

impl DirtySnapshot {
    /// `None` outside a git work tree or when git is unavailable.
    pub(crate) async fn take(root: &Path) -> Option<Self> {
        let top = git(root, &["rev-parse", "--show-toplevel"]).await?;
        let top = PathBuf::from(String::from_utf8_lossy(&top).trim());
        let status = git(root, &["status", "--porcelain=v1", "-z", "--untracked-files=all"]).await?;
        let paths: Vec<PathBuf> = porcelain_paths(&status).into_iter().map(|p| top.join(p)).collect();
        tokio::task::spawn_blocking(move || Self {
            files: paths
                .into_iter()
                .map(|path| {
                    let print = fingerprint(&path);
                    (path, print)
                })
                .collect(),
        })
        .await
        .ok()
    }

    /// Files whose content differs between the snapshots, including ones that became clean or were deleted.
    pub(crate) fn changes_since(&self, before: &Self, root: &Path) -> BTreeSet<WorkspaceChange> {
        let paths: BTreeSet<&PathBuf> = before.files.keys().chain(self.files.keys()).collect();
        paths
            .into_iter()
            .filter(|path| before.files.get(*path) != self.files.get(*path))
            .map(|path| WorkspaceChange {
                path: display(root, path),
                deleted: !path.exists(),
            })
            .collect()
    }
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
    fn porcelain_entries_include_both_sides_of_a_rename() {
        let stdout = b" M src/a.rs\0R  new.rs\0old.rs\0?? notes.md\0";
        assert_eq!(porcelain_paths(stdout), ["src/a.rs", "new.rs", "old.rs", "notes.md"]);
    }

    #[test]
    fn stale_paths_are_those_whose_content_changed_or_appeared() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("same.rs"), "a").unwrap();
        std::fs::write(dir.path().join("edited.rs"), "a").unwrap();
        let names: Vec<String> = ["same.rs", "edited.rs", "later.rs"].map(String::from).to_vec();
        let recorded = versions(dir.path(), &names);

        std::fs::write(dir.path().join("edited.rs"), "b").unwrap();
        std::fs::write(dir.path().join("later.rs"), "new").unwrap();

        assert_eq!(stale_paths(dir.path(), &recorded), ["edited.rs", "later.rs"]);
    }

    #[tokio::test]
    async fn snapshots_report_edits_new_files_reverts_and_deletions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let git_cmd = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
        };
        git_cmd(&["init", "--quiet", "-b", "main"]);
        for file in ["kept.rs", "reverted.rs", "gone.rs", "edited.rs"] {
            std::fs::write(root.join(file), "v1").unwrap();
        }
        git_cmd(&["add", "."]);
        git_cmd(&[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "--quiet",
            "-m",
            "init",
        ]);
        std::fs::write(root.join("reverted.rs"), "dirty").unwrap();
        std::fs::write(root.join("edited.rs"), "dirty").unwrap();
        let before = DirtySnapshot::take(&root).await.unwrap();

        std::fs::write(root.join("reverted.rs"), "v1").unwrap();
        std::fs::write(root.join("edited.rs"), "dirtier").unwrap();
        std::fs::remove_file(root.join("gone.rs")).unwrap();
        std::fs::write(root.join("new.rs"), "x").unwrap();
        let after = DirtySnapshot::take(&root).await.unwrap();

        let labels: Vec<String> = after
            .changes_since(&before, &root)
            .iter()
            .map(WorkspaceChange::label)
            .collect();
        assert_eq!(labels, ["edited.rs", "gone.rs (deleted)", "new.rs", "reverted.rs"]);
        assert!(
            DirtySnapshot::take(&std::env::temp_dir().join("no-such-dir"))
                .await
                .is_none()
        );
    }
}
