use std::path::{Path, PathBuf};
use std::time::Duration;

const GIT_TIMEOUT: Duration = Duration::from_secs(120);
const IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "rho"),
    ("GIT_AUTHOR_EMAIL", "rho@localhost"),
    ("GIT_COMMITTER_NAME", "rho"),
    ("GIT_COMMITTER_EMAIL", "rho@localhost"),
];

/// Runs git in `cwd` with `env`; stdout on success, the trimmed stderr on failure. A run that
/// outlasts `GIT_TIMEOUT` is killed and fails.
pub(crate) async fn run(cwd: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<Vec<u8>, String> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .envs(env.iter().copied())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(GIT_TIMEOUT, output)
        .await
        .map_err(|_| format!("`git {}` timed out after {}s", args.join(" "), GIT_TIMEOUT.as_secs()))?
        .map_err(|e| format!("could not run git: {e}"))?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("`git {}` failed: {}", args.join(" "), stderr.trim()))
}

pub(crate) async fn text(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let out = run(cwd, args, &[]).await?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

pub(crate) fn nul_fields(out: &[u8]) -> Vec<String> {
    out.split(|b| *b == 0)
        .map(|f| String::from_utf8_lossy(f).into_owned())
        .collect()
}

/// The repository facts every worktree operation starts from.
pub(crate) struct Repo {
    pub top: PathBuf,
    pub common_dir: PathBuf,
    pub(crate) git_dir: PathBuf,
    pub(crate) head: Option<String>,
}

fn parse_repo(out: &str) -> Option<Repo> {
    let mut lines = out.lines().map(str::trim);
    Some(Repo {
        top: PathBuf::from(lines.next()?),
        common_dir: PathBuf::from(lines.next()?),
        git_dir: PathBuf::from(lines.next()?),
        head: lines.next().map(str::to_string),
    })
}

/// The working tree containing `root`, in one git call; `None` outside git.
pub(crate) async fn repo(root: &Path) -> Option<Repo> {
    let mut args = vec![
        "rev-parse",
        "--path-format=absolute",
        "--show-toplevel",
        "--git-common-dir",
        "--absolute-git-dir",
        "HEAD",
    ];
    if let Ok(out) = text(root, &args).await {
        return parse_repo(&out);
    }
    args.pop();
    parse_repo(&text(root, &args).await.ok()?)
}

/// Commits `tree` with rho's identity, no hooks, and no signing.
pub(crate) async fn commit_tree(
    root: &Path,
    tree: &str,
    parent: Option<&str>,
    message: &str,
) -> Result<String, String> {
    let mut args = vec!["commit-tree", "--no-gpg-sign", tree, "-m", message];
    if let Some(parent) = parent {
        args.extend(["-p", parent]);
    }
    let out = run(root, &args, &IDENTITY).await?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

/// A commit of everything in `root`'s working tree (tracked and untracked, minus ignored files), made
/// through a copy of the index so the user's index and files are untouched.
pub(crate) async fn snapshot(root: &Path) -> Result<String, String> {
    let repo = repo(root)
        .await
        .ok_or_else(|| format!("{} is not in a git repository", root.display()))?;
    snapshot_repo(&repo).await
}

/// Removes the scratch index however `snapshot_repo` ends: an error, or its future being dropped.
struct ScratchIndex(PathBuf);

impl Drop for ScratchIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub(crate) async fn snapshot_repo(repo: &Repo) -> Result<String, String> {
    let scratch = std::env::temp_dir().join(format!("rho-index-{}", uuid::Uuid::new_v4().simple()));
    snapshot_through_index(repo, ScratchIndex(scratch)).await
}

async fn snapshot_through_index(repo: &Repo, scratch: ScratchIndex) -> Result<String, String> {
    let root = repo.top.as_path();
    let index = repo.git_dir.join("index");
    if index.exists() {
        tokio::fs::copy(&index, &scratch.0)
            .await
            .map_err(|e| format!("could not copy the git index: {e}"))?;
    }
    let scratch_text = scratch.0.to_string_lossy().into_owned();
    let env = [("GIT_INDEX_FILE", scratch_text.as_str())];
    let tree = async {
        run(root, &["add", "--all", "--", "."], &env).await?;
        let out = run(root, &["write-tree"], &env).await?;
        Ok::<String, String>(String::from_utf8_lossy(&out).trim().to_string())
    }
    .await;
    drop(scratch);
    commit_tree(root, &tree?, repo.head.as_deref(), "rho snapshot").await
}

/// Paths that differ between two commits, as `(status letter, path)` without rename detection.
pub(crate) async fn changed_paths(root: &Path, from: &str, to: &str) -> Result<Vec<(char, String)>, String> {
    let out = run(
        root,
        &["diff", "--name-status", "--no-renames", "-z", from, to, "--"],
        &[],
    )
    .await?;
    let fields = nul_fields(&out);
    Ok(fields
        .chunks(2)
        .filter_map(|pair| match pair {
            [status, path] if !path.is_empty() => Some((status.chars().next().unwrap_or('M'), path.clone())),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
pub(crate) mod test_repo {
    use crate::test_support::TestDir;
    use std::path::{Path, PathBuf};

    /// A throwaway repository with one commit of `files`, using no user or global git config.
    pub(crate) fn create(files: &[(&str, &str)]) -> (TestDir, PathBuf) {
        let dir = TestDir::new();
        let root = dir.join("repo");
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        git(&root, &["init", "--quiet", "-b", "main"]);
        for (path, body) in files {
            write(&root, path, body);
        }
        git(&root, &["add", "."]);
        git(&root, &["commit", "--quiet", "--allow-empty", "-m", "init"]);
        (dir, root)
    }

    pub(crate) fn write(root: &Path, path: &str, body: &str) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    pub(crate) fn git(root: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args([
                "-c",
                "user.email=t@e",
                "-c",
                "user.name=t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::test_repo::{create, git, write};
    use super::*;

    #[tokio::test]
    async fn snapshots_capture_uncommitted_files_without_touching_the_index() {
        let (_dir, root) = create(&[("a.rs", "v1"), ("gone.rs", "x"), (".gitignore", "/target\n")]);
        write(&root, "a.rs", "v2");
        write(&root, "new.rs", "n");
        write(&root, "target/out", "ignored");
        std::fs::remove_file(root.join("gone.rs")).unwrap();
        let status_before = git(&root, &["status", "--porcelain"]);

        let commit = snapshot(&root).await.unwrap();

        assert_eq!(git(&root, &["show", &format!("{commit}:a.rs")]), "v2");
        assert_eq!(git(&root, &["show", &format!("{commit}:new.rs")]), "n");
        let head = git(&root, &["rev-parse", "HEAD"]);
        let mut changes = changed_paths(&root, &head, &commit).await.unwrap();
        changes.sort();
        assert_eq!(
            changes,
            [('A', "new.rs".into()), ('D', "gone.rs".into()), ('M', "a.rs".into())]
        );
        assert_eq!(git(&root, &["status", "--porcelain"]), status_before);
    }

    #[tokio::test]
    async fn a_snapshot_cancelled_mid_flight_removes_its_scratch_index() {
        let (_dir, root) = create(&[("a.rs", "v1")]);
        let scratch_dir = rho_test_support::TestDir::new();
        let scratch = scratch_dir.join("scratch-index");
        let repo = repo(&root).await.unwrap();
        let task = tokio::spawn({
            let scratch = ScratchIndex(scratch.clone());
            async move { snapshot_through_index(&repo, scratch).await }
        });
        while !scratch.exists() {
            tokio::task::yield_now().await;
        }

        task.abort();
        let _ = task.await;

        assert!(!scratch.exists());
    }
}
