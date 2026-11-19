use super::FileChange;
use super::git;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeOutcome {
    /// Files written or deleted in the target.
    pub changed: Vec<FileChange>,
    /// Files left with conflict markers (or, for add/delete conflicts, the merged side) to resolve.
    pub conflicts: Vec<String>,
}

impl MergeOutcome {
    #[must_use]
    pub fn summary(&self) -> String {
        let changed: Vec<String> = self.changed.iter().map(FileChange::label).collect();
        let mut text = if changed.is_empty() {
            "No changes to merge.".to_string()
        } else {
            format!("Merged {} file(s): {}.", changed.len(), changed.join(", "))
        };
        if !self.conflicts.is_empty() {
            text.push_str(&format!(
                " Conflicts to resolve (look for <<<<<<< markers): {}.",
                self.conflicts.join(", ")
            ));
        }
        text
    }
}

/// `git merge-tree` of `ours` and `theirs` over `base`: the result tree and the conflicted paths.
async fn merge_tree(root: &Path, base: &str, ours: &str, theirs: &str) -> Result<(String, Vec<String>), String> {
    let merge_base = format!("--merge-base={base}");
    let output = tokio::process::Command::new("git")
        .args([
            "merge-tree",
            "--write-tree",
            "-z",
            "--name-only",
            &merge_base,
            ours,
            theirs,
        ])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| format!("could not run git: {e}"))?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`git merge-tree` failed: {}", stderr.trim()));
    }
    let mut fields = git::nul_fields(&output.stdout).into_iter();
    let tree = fields.next().unwrap_or_default();
    let mut conflicts: Vec<String> = fields.take_while(|f| !f.is_empty()).collect();
    conflicts.dedup();
    Ok((tree, conflicts))
}

/// `path -> (mode, blob id)` for `paths` in `tree`, from one `ls-tree` call.
async fn tree_entries(root: &Path, tree: &str, paths: &[&str]) -> Result<HashMap<String, (String, String)>, String> {
    let mut args = vec!["ls-tree", "-z", tree, "--"];
    args.extend_from_slice(paths);
    let out = git::run(root, &args, &[]).await?;
    Ok(git::nul_fields(&out)
        .into_iter()
        .filter_map(|entry| {
            let (meta, path) = entry.split_once('\t')?;
            let mut parts = meta.split_whitespace();
            let mode = parts.next()?.to_string();
            let oid = parts.nth(1)?.to_string();
            Some((path.to_string(), (mode, oid)))
        })
        .collect())
}

fn is_regular_file(path: &Path) -> bool {
    path.symlink_metadata().is_ok_and(|m| m.is_file())
}

/// Fails if any regular file among `paths` no longer matches `ours`, i.e. it was edited while the merge ran.
async fn ensure_unchanged(target: &Path, ours: &str, paths: &[&str]) -> Result<(), String> {
    let expected = tree_entries(target, ours, paths).await?;
    let on_disk: Vec<&str> = paths
        .iter()
        .copied()
        .filter(|p| is_regular_file(&target.join(p)))
        .collect();
    let hashes: Vec<String> = if on_disk.is_empty() {
        Vec::new()
    } else {
        let mut args = vec!["hash-object", "--"];
        args.extend_from_slice(&on_disk);
        let out = git::run(target, &args, &[]).await?;
        String::from_utf8_lossy(&out).lines().map(str::to_string).collect()
    };
    let now: HashMap<&str, &str> = on_disk.iter().copied().zip(hashes.iter().map(String::as_str)).collect();
    let moved = paths.iter().find(|path| {
        let was = expected
            .get(**path)
            .filter(|(mode, _)| mode != "120000")
            .map(|(_, oid)| oid.as_str());
        let symlink = target
            .join(path)
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink());
        !symlink && was != now.get(**path).copied()
    });
    match moved {
        Some(path) => Err(format!(
            "{path} changed while merging; nothing was written, merge again"
        )),
        None => Ok(()),
    }
}

async fn write_entry(root: &Path, path: &str, entry: Option<&(String, String)>) -> Result<(), String> {
    let (mode, oid) = entry.ok_or_else(|| format!("{path} is missing from the merge result"))?;
    let bytes = git::run(root, &["cat-file", "blob", oid], &[]).await?;
    let file = root.join(path);
    if let Some(parent) = file.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    if file.symlink_metadata().is_ok() {
        tokio::fs::remove_file(&file).await.map_err(|e| e.to_string())?;
    }
    if mode == "120000" {
        let target = String::from_utf8_lossy(&bytes).into_owned();
        return symlink(Path::new(&target), &file).map_err(|e| format!("could not write {path}: {e}"));
    }
    tokio::fs::write(&file, &bytes)
        .await
        .map_err(|e| format!("could not write {path}: {e}"))?;
    set_executable(&file, mode == "100755").await
}

/// Writes or deletes each changed path in `target` to match `tree`.
async fn apply(target: &Path, tree: &str, changes: &[(char, String)]) -> Result<(), String> {
    let written: Vec<&str> = changes
        .iter()
        .filter(|(status, _)| *status != 'D')
        .map(|(_, path)| path.as_str())
        .collect();
    let entries = if written.is_empty() {
        HashMap::new()
    } else {
        tree_entries(target, tree, &written).await?
    };
    for (status, path) in changes {
        if *status != 'D' {
            write_entry(target, path, entries.get(path)).await?;
            continue;
        }
        match tokio::fs::remove_file(target.join(path)).await {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("could not delete {path}: {e}")),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(unix)]
async fn set_executable(file: &Path, executable: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    tokio::fs::set_permissions(file, std::fs::Permissions::from_mode(mode))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
async fn set_executable(_file: &Path, _executable: bool) -> Result<(), String> {
    Ok(())
}

/// Merges write into working trees; one at a time so two agents finishing together cannot interleave.
static MERGE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Applies `base..theirs` to `target`'s working tree with a three-way merge against its current,
/// possibly uncommitted, state. The index is untouched; files changed during the merge abort it.
pub(crate) async fn merge(target: &Path, base: &str, theirs: &str) -> Result<MergeOutcome, String> {
    let _guard = MERGE_LOCK.lock().await;
    let repo = git::repo(target)
        .await
        .ok_or("the merge target is not a git working tree")?;
    let ours = git::snapshot_repo(&repo).await?;
    let target = repo.top;
    let (tree, conflicts) = merge_tree(&target, base, &ours, theirs).await?;
    let changes = git::changed_paths(&target, &ours, &tree).await?;
    if !changes.is_empty() {
        let paths: Vec<&str> = changes.iter().map(|(_, path)| path.as_str()).collect();
        ensure_unchanged(&target, &ours, &paths).await?;
        apply(&target, &tree, &changes).await?;
    }
    Ok(MergeOutcome {
        changed: changes
            .into_iter()
            .map(|(status, path)| FileChange { status, path })
            .collect(),
        conflicts,
    })
}

#[cfg(test)]
mod tests {
    use super::super::WorktreeStore;
    use super::super::git::test_repo::{create, git, write};

    #[tokio::test]
    async fn clean_merges_land_on_a_dirty_tree_and_overlaps_leave_markers() {
        let (dir, root) = create(&[
            ("a.rs", "1\n2\n3\n"),
            ("b.rs", "x\n"),
            ("gone.rs", "g\n"),
            ("run.sh", "echo\n"),
        ]);
        write(&root, "notes.md", "mine, uncommitted\n");
        let store = WorktreeStore::new(dir.path().join("store"));
        let wt = store.create(&root, "agent").await.unwrap().unwrap();
        write(&wt.path, "a.rs", "1\nAGENT\n3\n");
        write(&wt.path, "new.rs", "n\n");
        write(&wt.path, "b.rs", "x\nagent line\n");
        std::fs::remove_file(wt.path.join("gone.rs")).unwrap();
        write(&root, "a.rs", "1\nPARENT\n3\n");

        let outcome = wt.merge_into(&root).await.unwrap();

        assert_eq!(outcome.conflicts, ["a.rs"]);
        let a = std::fs::read_to_string(root.join("a.rs")).unwrap();
        assert!(
            a.contains("<<<<<<<") && a.contains("PARENT") && a.contains("AGENT"),
            "{a}"
        );
        assert_eq!(std::fs::read_to_string(root.join("b.rs")).unwrap(), "x\nagent line\n");
        assert_eq!(std::fs::read_to_string(root.join("new.rs")).unwrap(), "n\n");
        assert!(!root.join("gone.rs").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("notes.md")).unwrap(),
            "mine, uncommitted\n"
        );
        assert_eq!(
            outcome.summary(),
            "Merged 4 file(s): a.rs, b.rs, gone.rs (deleted), new.rs (added). Conflicts to resolve (look for <<<<<<< markers): a.rs."
        );
        assert!(!wt.path.exists(), "a merged worktree is removed");
        assert!(
            git(&root, &["diff", "--cached", "--name-only"]).is_empty(),
            "the index is untouched"
        );
    }
}
