use super::workspace::{Fingerprint, WorkspaceChange, display, fingerprint};
use crate::tools::traversal::walker_builder;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WALK_BUDGET: Duration = Duration::from_millis(150);
const MAX_WALKED_FILES: usize = 20_000;

/// The workspace files that could change, with fingerprints: uncommitted files in a git work tree
/// (by content), or every non-ignored file found by a bounded walk elsewhere (by size and mtime).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    files: BTreeMap<PathBuf, Fingerprint>,
    complete: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Changes {
    pub files: BTreeSet<WorkspaceChange>,
    /// False when a walk hit its time or file limit, so the list may miss files.
    pub complete: bool,
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

fn nul_separated(stdout: &[u8]) -> impl Iterator<Item = String> + '_ {
    stdout
        .split(|b| *b == 0)
        .filter(|e| !e.is_empty())
        .map(|e| String::from_utf8_lossy(e).into_owned())
}

/// `(status code, path)` from `git status --porcelain=v1 -z`; a rename or copy also yields its original path.
fn porcelain_entries(stdout: &[u8]) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut fields = nul_separated(stdout).filter(|e| e.len() > 3);
    while let Some(entry) = fields.next() {
        let code = entry[..2].to_string();
        let renamed = code.contains(['R', 'C']);
        entries.push((code.clone(), entry[3..].to_string()));
        if renamed {
            entries.extend(fields.next().map(|orig| (code.clone(), orig)));
        }
    }
    entries
}

/// Uncommitted paths of the git work tree containing `root`; `None` outside git.
async fn git_status(root: &Path) -> Option<Vec<PathBuf>> {
    let top = git(root, &["rev-parse", "--show-toplevel"]).await?;
    let top = PathBuf::from(String::from_utf8_lossy(&top).trim());
    let status = git(root, &["status", "--porcelain=v1", "-z", "--untracked-files=all"]).await?;
    Some(
        porcelain_entries(&status)
            .into_iter()
            .map(|(_, path)| top.join(path))
            .collect(),
    )
}

fn stat_print(path: &Path) -> Fingerprint {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    meta.is_file()
        .then(|| format!("{}:{}", meta.len(), modified.as_nanos()))
}

fn walk(root: &Path, extra: Vec<PathBuf>) -> Snapshot {
    let deadline = Instant::now() + WALK_BUDGET;
    let mut files = BTreeMap::new();
    let mut complete = true;
    for entry in walker_builder(root, false, None).build().flatten() {
        if Instant::now() >= deadline || files.len() >= MAX_WALKED_FILES {
            complete = false;
            break;
        }
        if entry.file_type().is_some_and(|t| t.is_file()) {
            files.insert(entry.path().to_path_buf(), stat_print(entry.path()));
        }
    }
    for path in extra {
        let print = stat_print(&path);
        files.entry(path).or_insert(print);
    }
    Snapshot { files, complete }
}

async fn fingerprints(paths: BTreeSet<PathBuf>) -> BTreeMap<PathBuf, Fingerprint> {
    tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|path| {
                let print = fingerprint(&path);
                (path, print)
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

impl Snapshot {
    /// Snapshots `root`, also fingerprinting the files in `previous` so a file that became clean
    /// (committed or reverted) compares by content. `None` when `root` is not a directory.
    pub(crate) async fn take(root: &Path, previous: Option<&Self>) -> Option<Self> {
        if !root.is_dir() {
            return None;
        }
        let extra: Vec<PathBuf> = previous.map(|p| p.files.keys().cloned().collect()).unwrap_or_default();
        let Some(status) = git_status(root).await else {
            let root = root.to_path_buf();
            return tokio::task::spawn_blocking(move || walk(&root, extra)).await.ok();
        };
        let paths: BTreeSet<PathBuf> = status.into_iter().chain(extra).collect();
        Some(Self {
            files: fingerprints(paths).await,
            complete: true,
        })
    }

    pub(crate) fn changes_since(&self, before: &Self, root: &Path) -> Changes {
        let paths: BTreeSet<&PathBuf> = before.files.keys().chain(self.files.keys()).collect();
        let files = paths
            .into_iter()
            .filter(|path| before.files.get(*path) != self.files.get(*path))
            .map(|path| change(root, path))
            .collect();
        Changes {
            files,
            complete: self.complete && before.complete,
        }
    }
}

fn change(root: &Path, path: &Path) -> WorkspaceChange {
    WorkspaceChange {
        path: display(root, path),
        deleted: !path.exists(),
        by: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(changes: &BTreeSet<WorkspaceChange>) -> Vec<String> {
        changes.iter().map(WorkspaceChange::label).collect()
    }

    fn repo() -> (tempfile::TempDir, PathBuf, impl Fn(&[&str])) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let cwd = root.clone();
        let git_cmd = move |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(["-c", "user.email=t@e", "-c", "user.name=t"])
                .args(args)
                .current_dir(&cwd)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            assert!(status.status.success(), "git {args:?}");
        };
        git_cmd(&["init", "--quiet", "-b", "main"]);
        for file in ["kept.rs", "reverted.rs", "gone.rs", "edited.rs", "committed.rs"] {
            std::fs::write(root.join(file), "v1").unwrap();
        }
        git_cmd(&["add", "."]);
        git_cmd(&["commit", "--quiet", "-m", "init"]);
        (dir, root, git_cmd)
    }

    #[test]
    fn porcelain_entries_include_both_sides_of_a_rename() {
        let stdout = b" M src/a.rs\0R  new.rs\0old.rs\0?? notes.md\0";
        let paths: Vec<String> = porcelain_entries(stdout).into_iter().map(|(_, p)| p).collect();
        assert_eq!(paths, ["src/a.rs", "new.rs", "old.rs", "notes.md"]);
    }

    #[tokio::test]
    async fn git_snapshots_report_content_changes_but_not_commits() {
        let (_dir, root, git_cmd) = repo();
        for file in ["reverted.rs", "edited.rs", "committed.rs"] {
            std::fs::write(root.join(file), "dirty").unwrap();
        }
        let before = Snapshot::take(&root, None).await.unwrap();

        std::fs::write(root.join("reverted.rs"), "v1").unwrap();
        std::fs::write(root.join("edited.rs"), "dirtier").unwrap();
        std::fs::remove_file(root.join("gone.rs")).unwrap();
        std::fs::write(root.join("new.rs"), "x").unwrap();
        git_cmd(&["commit", "--quiet", "-m", "wip", "--", "committed.rs"]);
        let after = Snapshot::take(&root, Some(&before)).await.unwrap();

        let changes = after.changes_since(&before, &root);
        assert!(changes.complete);
        assert_eq!(
            labels(&changes.files),
            ["edited.rs", "gone.rs (deleted)", "new.rs", "reverted.rs"]
        );
    }

    #[tokio::test]
    async fn outside_git_a_bounded_walk_finds_edits_new_files_and_deletions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("a.rs"), "1").unwrap();
        std::fs::write(root.join("b.rs"), "1").unwrap();
        let before = Snapshot::take(&root, None).await.unwrap();

        std::fs::write(root.join("a.rs"), "22").unwrap();
        std::fs::remove_file(root.join("b.rs")).unwrap();
        std::fs::write(root.join("c.rs"), "1").unwrap();
        let after = Snapshot::take(&root, Some(&before)).await.unwrap();

        let changes = after.changes_since(&before, &root);
        assert!(changes.complete);
        assert_eq!(labels(&changes.files), ["a.rs", "b.rs (deleted)", "c.rs"]);
        assert!(Snapshot::take(&root.join("missing"), None).await.is_none());
    }
}
