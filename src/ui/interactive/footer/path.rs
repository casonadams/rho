use super::super::{GitPrInfo, GitStatusCounts};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub fn abbreviate_home(cwd: &Path, home: Option<&Path>) -> String {
    let Some(home) = home else {
        return cwd.display().to_string();
    };
    if cwd == home {
        return "~".to_string();
    }
    if let Ok(rel) = cwd.strip_prefix(home) {
        let rel_str = rel.to_string_lossy();
        if rel_str.is_empty() {
            return "~".to_string();
        }
        return format!("~/{rel_str}");
    }
    cwd.display().to_string()
}

fn branch_from_head_file(head_file: &Path) -> Option<String> {
    let head_content = std::fs::read_to_string(head_file).ok()?;
    head_content.trim().strip_prefix("ref: refs/heads/").map(str::to_string)
}

fn branch_from_git_dir(dir: &Path, git_dir: &Path) -> Option<String> {
    let head_file = git_dir.join("HEAD");
    if git_dir.is_dir() {
        return branch_from_head_file(&head_file);
    }
    if git_dir.is_file() {
        let content = std::fs::read_to_string(git_dir).ok()?;
        let gitdir_path = content.trim().strip_prefix("gitdir:")?;
        let gitdir = PathBuf::from(gitdir_path.trim());
        let resolved = if gitdir.is_absolute() { gitdir } else { dir.join(gitdir) };
        return branch_from_head_file(&resolved.join("HEAD"));
    }
    None
}

fn branch_from_git_process(cwd: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("branch")
        .arg("--show-current")
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty()).then_some(branch)
}

pub async fn get_git_branch_async(cwd: PathBuf) -> Option<String> {
    tokio::task::spawn_blocking(move || get_git_branch(&cwd))
        .await
        .ok()
        .flatten()
}

pub fn get_git_branch(cwd: &Path) -> Option<String> {
    let mut curr = Some(cwd);
    while let Some(dir) = curr {
        let git_dir = dir.join(".git");
        if git_dir.is_dir() || git_dir.is_file() {
            if let Some(branch) = branch_from_git_dir(dir, &git_dir) {
                return Some(branch);
            }
            break;
        }
        curr = dir.parent();
    }

    branch_from_git_process(cwd)
}

pub fn parse_git_status_porcelain(stdout: &str) -> GitStatusCounts {
    let mut counts = GitStatusCounts::default();
    for line in stdout.lines() {
        let bytes = line.as_bytes();
        if bytes.len() < 2 {
            continue;
        }
        let (x, y) = (bytes[0], bytes[1]);
        if x == b'?' && y == b'?' {
            counts.untracked += 1;
            continue;
        }
        if x != b' ' && x != b'!' {
            counts.staged += 1;
        }
        if y != b' ' && y != b'!' {
            counts.unstaged += 1;
        }
    }
    counts
}

pub fn get_git_status_counts(cwd: &Path) -> Option<GitStatusCounts> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(parse_git_status_porcelain(&stdout))
}

pub async fn get_git_status_counts_async(cwd: PathBuf) -> Option<GitStatusCounts> {
    tokio::task::spawn_blocking(move || get_git_status_counts(&cwd))
        .await
        .ok()
        .flatten()
}

pub fn parse_gh_pr_json(stdout: &str) -> Option<GitPrInfo> {
    let value: serde_json::Value = serde_json::from_str(stdout).ok()?;
    let number = value.get("number")?.as_u64()?;
    let url = value
        .get("url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some(GitPrInfo { number, url })
}

pub fn parse_github_repo(remote_url: &str) -> Option<String> {
    let trimmed = remote_url.trim();
    let after_host = trimmed
        .split_once("github.com:")
        .or_else(|| trimmed.split_once("github.com/"))?
        .1;
    let cleaned = after_host.trim_end_matches('/').trim_end_matches(".git");
    let (owner, repo) = cleaned.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

#[cfg(not(test))]
fn resolve_github_repo(cwd: &Path, branch: &str) -> Option<String> {
    let remote = std::process::Command::new("git")
        .args(["config", "--get", &format!("branch.{branch}.remote")])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "origin".to_string());

    let url = std::process::Command::new("git")
        .args(["remote", "get-url", &remote])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;
    parse_github_repo(&url)
}

#[cfg(not(test))]
fn run_gh_pr_view(cwd: &Path, extra_args: &[&str]) -> Option<GitPrInfo> {
    let output = std::process::Command::new("gh")
        .env("GH_PROMPT_DISABLED", "1")
        .arg("pr")
        .arg("view")
        .args(extra_args)
        .args(["--json", "number,url"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_gh_pr_json(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(test))]
fn fetch_git_pr(cwd: &Path, branch: &str) -> Option<GitPrInfo> {
    if let Some(pr) = run_gh_pr_view(cwd, &[]) {
        return Some(pr);
    }
    let repo = resolve_github_repo(cwd, branch)?;
    run_gh_pr_view(cwd, &[branch, "--repo", &repo])
}

const PR_MISS_TTL: Duration = Duration::from_secs(60);

pub fn is_default_branch(branch: &str) -> bool {
    matches!(branch, "main" | "master")
}

#[derive(Default)]
struct PrCacheEntry {
    key: Option<(PathBuf, String)>,
    pr: Option<GitPrInfo>,
    fetched_at: Option<Instant>,
    in_flight: bool,
}

static PR_CACHE: OnceLock<Mutex<PrCacheEntry>> = OnceLock::new();
static PR_VERSION: OnceLock<tokio::sync::watch::Sender<u64>> = OnceLock::new();

fn pr_version_sender() -> &'static tokio::sync::watch::Sender<u64> {
    PR_VERSION.get_or_init(|| tokio::sync::watch::channel(0).0)
}

pub fn subscribe_git_pr() -> tokio::sync::watch::Receiver<u64> {
    pr_version_sender().subscribe()
}

fn cache_entry_is_fresh(entry: &PrCacheEntry, key: &(PathBuf, String), now: Instant) -> bool {
    if entry.key.as_ref() != Some(key) {
        return false;
    }
    if entry.pr.is_some() {
        return true;
    }
    entry
        .fetched_at
        .is_some_and(|t| now.saturating_duration_since(t) < PR_MISS_TTL)
}

/// Non-blocking PR lookup cached per `(cwd, branch)`. Spawns a background fetch on miss
/// (with a 60s TTL on `None` misses) and notifies `subscribe_git_pr()` when complete.
pub fn cached_git_pr(cwd: &Path, branch: Option<&str>) -> Option<GitPrInfo> {
    let branch = branch.filter(|b| !b.is_empty() && !is_default_branch(b))?;
    let key = (cwd.to_path_buf(), branch.to_string());
    let cache = PR_CACHE.get_or_init(|| Mutex::new(PrCacheEntry::default()));
    let mut guard = cache.lock().ok()?;
    let now = Instant::now();
    if cache_entry_is_fresh(&guard, &key, now) {
        return guard.pr.clone();
    }
    if guard.in_flight {
        return None;
    }
    guard.key = Some(key.clone());
    guard.pr = None;
    guard.fetched_at = Some(now);
    #[cfg(not(test))]
    {
        guard.in_flight = true;
        drop(guard);
        let cwd_owned = key.0.clone();
        let branch_owned = key.1.clone();
        std::thread::spawn(move || {
            let pr = fetch_git_pr(&cwd_owned, &branch_owned);
            let updated = if let Ok(mut g) = cache.lock() {
                let matched = g.key.as_ref() == Some(&key);
                if matched {
                    g.pr = pr;
                    g.fetched_at = Some(Instant::now());
                }
                g.in_flight = false;
                matched
            } else {
                false
            };
            if updated {
                pr_version_sender().send_modify(|v| *v = v.wrapping_add(1));
            }
        });
    }
    None
}
