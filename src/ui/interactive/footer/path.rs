use super::super::{GitPrInfo, GitStatusCounts};
use super::segments::status_bar_uses_segment;
use rho_harness_core::config::{StatusBarConfig, StatusBarSegmentId};
use rho_harness_core::locking::lock;
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

use crate::platform::git::run_command_with_timeout;
pub use crate::platform::git::{get_git_branch, get_git_branch_async};

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
        if matches!((x, y), (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D')) {
            counts.unstaged += 1;
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

const GIT_STATUS_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(not(test))]
const GH_TIMEOUT: Duration = Duration::from_secs(5);

pub fn get_git_status_counts(cwd: &Path) -> Option<GitStatusCounts> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["status", "--porcelain=v1"]).current_dir(cwd);
    let output = run_command_with_timeout(cmd, GIT_STATUS_TIMEOUT)?;
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
    let obj = match &value {
        serde_json::Value::Array(items) => items.first()?,
        serde_json::Value::Object(_) => &value,
        _ => return None,
    };
    let number = obj.get("number")?.as_u64()?;
    let url = obj
        .get("url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some(GitPrInfo { number, url })
}

fn extract_github_path(trimmed: &str) -> Option<&str> {
    if let Some(rest) = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .or_else(|| trimmed.strip_prefix("ssh://"))
    {
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        return host.eq_ignore_ascii_case("github.com").then_some(path);
    }
    let (user_host, path) = trimmed.split_once(':')?;
    let host = user_host.rsplit_once('@').map_or(user_host, |(_, h)| h);
    host.eq_ignore_ascii_case("github.com").then_some(path)
}

pub fn parse_github_repo(remote_url: &str) -> Option<String> {
    let path = extract_github_path(remote_url.trim())?;
    let without_slash = path.trim_end_matches('/');
    let cleaned = without_slash.strip_suffix(".git").unwrap_or(without_slash);
    let (owner, repo) = cleaned.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

#[cfg(not(test))]
fn resolve_github_repo(cwd: &Path, branch: &str) -> Option<String> {
    let mut remote_cmd = std::process::Command::new("git");
    remote_cmd
        .args(["config", "--get", &format!("branch.{branch}.remote")])
        .current_dir(cwd);
    let remote = run_command_with_timeout(remote_cmd, GH_TIMEOUT)
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "origin".to_string());

    let mut url_cmd = std::process::Command::new("git");
    url_cmd.args(["remote", "get-url", "--", &remote]).current_dir(cwd);
    let url = run_command_with_timeout(url_cmd, GH_TIMEOUT)
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;
    parse_github_repo(&url)
}

#[cfg(not(test))]
fn run_gh_pr_view(cwd: &Path, branch: &str, repo: Option<&str>) -> Option<GitPrInfo> {
    let mut cmd = std::process::Command::new("gh");
    cmd.env("GH_PROMPT_DISABLED", "1")
        .args([
            "pr",
            "list",
            &format!("--head={branch}"),
            "--limit",
            "1",
            "--json",
            "number,url",
        ])
        .current_dir(cwd);
    if let Some(repo) = repo {
        cmd.args(["--repo", repo]);
    }
    let output = run_command_with_timeout(cmd, GH_TIMEOUT)?;
    if !output.status.success() {
        return None;
    }
    parse_gh_pr_json(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(test))]
fn fetch_git_pr(cwd: &Path, branch: &str) -> Option<GitPrInfo> {
    if let Some(pr) = run_gh_pr_view(cwd, branch, None) {
        return Some(pr);
    }
    let repo = resolve_github_repo(cwd, branch)?;
    run_gh_pr_view(cwd, branch, Some(&repo))
}

const PR_HIT_TTL: Duration = Duration::from_secs(300);
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
    let ttl = if entry.pr.is_some() { PR_HIT_TTL } else { PR_MISS_TTL };
    entry.fetched_at.is_some_and(|t| now.saturating_duration_since(t) < ttl)
}

/// Non-blocking PR lookup cached per `(cwd, branch)`. Spawns a background fetch on miss
/// (with a 300s TTL on hits and 60s TTL on `None` misses) and notifies `subscribe_git_pr()` when complete.
pub fn cached_git_pr(cwd: &Path, branch: Option<&str>) -> Option<GitPrInfo> {
    let branch = branch.filter(|b| !b.is_empty() && !is_default_branch(b))?;
    let key = (cwd.to_path_buf(), branch.to_string());
    let cache = PR_CACHE.get_or_init(|| Mutex::new(PrCacheEntry::default()));
    let mut guard = lock(cache);
    let now = Instant::now();
    if cache_entry_is_fresh(&guard, &key, now) {
        return guard.pr.clone();
    }
    let stale = if guard.key.as_ref() == Some(&key) {
        guard.pr.clone()
    } else {
        None
    };
    if guard.in_flight {
        return stale;
    }
    guard.key = Some(key.clone());
    guard.pr = stale.clone();
    guard.fetched_at = Some(now);
    #[cfg(not(test))]
    {
        guard.in_flight = true;
        drop(guard);
        let cwd_owned = key.0.clone();
        let branch_owned = key.1.clone();
        std::thread::spawn(move || {
            let pr = fetch_git_pr(&cwd_owned, &branch_owned);
            let updated = {
                let mut g = lock(cache);
                let matched = g.key.as_ref() == Some(&key);
                if matched {
                    g.pr = pr;
                    g.fetched_at = Some(Instant::now());
                }
                g.in_flight = false;
                matched
            };
            if updated {
                pr_version_sender().send_modify(|v| *v = v.wrapping_add(1));
            }
        });
    }
    stale
}

/// Whether a footer read may start a fresh `git status`; a running turn only reads the last one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitStatusRead {
    Refresh,
    Cached,
}

/// PR and git status for the enabled footer segments, read through the cached lookups above.
pub fn cached_footer_git(
    cfg: &StatusBarConfig,
    cwd: &Path,
    branch: Option<&str>,
    status_read: GitStatusRead,
) -> (Option<GitPrInfo>, Option<GitStatusCounts>) {
    let pr = status_bar_uses_segment(cfg, StatusBarSegmentId::Pr)
        .then(|| cached_git_pr(cwd, branch))
        .flatten();
    let git_status = status_bar_uses_segment(cfg, StatusBarSegmentId::Git)
        .then(|| match status_read {
            GitStatusRead::Refresh => cached_git_status_counts(cwd),
            GitStatusRead::Cached => last_git_status_counts(cwd),
        })
        .flatten();
    (pr, git_status)
}

const GIT_BRANCH_TTL: Duration = Duration::from_secs(30);

#[derive(Default)]
struct GitBranchCacheEntry {
    cwd: Option<PathBuf>,
    branch: Option<String>,
    fetched_at: Option<Instant>,
    in_flight: bool,
}

static GIT_BRANCH_CACHE: std::sync::LazyLock<Mutex<GitBranchCacheEntry>> =
    std::sync::LazyLock::new(|| Mutex::new(GitBranchCacheEntry::default()));

/// The branch of `cwd` without blocking: read from the repository's `HEAD` when it names one, else
/// the last answer from `git`, asked again in the background once stale and announced through
/// `subscribe_git_pr()`.
pub fn cached_git_branch(cwd: &Path) -> Option<String> {
    if let Some(branch) = crate::platform::git::branch_from_files(cwd) {
        return Some(branch);
    }
    let mut guard = lock(&GIT_BRANCH_CACHE);
    let now = Instant::now();
    let same_cwd = guard.cwd.as_deref() == Some(cwd);
    let stale = if same_cwd { guard.branch.clone() } else { None };
    let fresh = same_cwd
        && guard
            .fetched_at
            .is_some_and(|t| now.saturating_duration_since(t) < GIT_BRANCH_TTL);
    if fresh || guard.in_flight {
        return stale;
    }
    guard.cwd = Some(cwd.to_path_buf());
    guard.branch = stale.clone();
    guard.fetched_at = Some(now);
    #[cfg(not(test))]
    {
        guard.in_flight = true;
        drop(guard);
        let cwd_owned = cwd.to_path_buf();
        std::thread::spawn(move || {
            let branch = get_git_branch(&cwd_owned);
            let updated = {
                let mut g = lock(&GIT_BRANCH_CACHE);
                let matched = g.cwd.as_deref() == Some(cwd_owned.as_path());
                let changed = matched && g.branch != branch;
                if matched {
                    g.branch = branch;
                    g.fetched_at = Some(Instant::now());
                }
                g.in_flight = false;
                changed
            };
            if updated {
                pr_version_sender().send_modify(|v| *v = v.wrapping_add(1));
            }
        });
    }
    stale
}

const GIT_STATUS_TTL: Duration = Duration::from_secs(5);

#[derive(Default)]
struct GitStatusCacheEntry {
    cwd: Option<PathBuf>,
    counts: Option<GitStatusCounts>,
    fetched_at: Option<Instant>,
    in_flight: bool,
}

static GIT_STATUS_CACHE: std::sync::LazyLock<Mutex<GitStatusCacheEntry>> =
    std::sync::LazyLock::new(|| Mutex::new(GitStatusCacheEntry::default()));

fn git_status_entry_is_fresh(entry: &GitStatusCacheEntry, cwd: &Path, now: Instant) -> bool {
    entry.cwd.as_deref() == Some(cwd)
        && entry
            .fetched_at
            .is_some_and(|t| now.saturating_duration_since(t) < GIT_STATUS_TTL)
}

/// The git status last read for `cwd`, without starting a new read.
fn last_git_status_counts(cwd: &Path) -> Option<GitStatusCounts> {
    let guard = lock(&GIT_STATUS_CACHE);
    if guard.cwd.as_deref() == Some(cwd) {
        guard.counts
    } else {
        None
    }
}

/// Non-blocking git status lookup cached per `cwd`. Spawns a background fetch on miss/expiry
/// and notifies `subscribe_git_pr()` when complete.
pub fn cached_git_status_counts(cwd: &Path) -> Option<GitStatusCounts> {
    let mut guard = lock(&GIT_STATUS_CACHE);
    let now = Instant::now();
    if git_status_entry_is_fresh(&guard, cwd, now) {
        return guard.counts;
    }
    let stale = if guard.cwd.as_deref() == Some(cwd) {
        guard.counts
    } else {
        None
    };
    if guard.in_flight {
        return stale;
    }
    guard.cwd = Some(cwd.to_path_buf());
    guard.counts = stale;
    guard.fetched_at = Some(now);
    #[cfg(not(test))]
    {
        guard.in_flight = true;
        drop(guard);
        let cwd_owned = cwd.to_path_buf();
        std::thread::spawn(move || {
            let counts = get_git_status_counts(&cwd_owned);
            let updated = {
                let mut g = lock(&GIT_STATUS_CACHE);
                let matched = g.cwd.as_deref() == Some(cwd_owned.as_path());
                if matched {
                    g.counts = counts;
                    g.fetched_at = Some(Instant::now());
                }
                g.in_flight = false;
                matched
            };
            if updated {
                pr_version_sender().send_modify(|v| *v = v.wrapping_add(1));
            }
        });
    }
    stale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gh_pr_json_supports_array_and_object() {
        let arr = r#"[{"number": 72, "url": "https://github.com/org/repo/pull/72"}]"#;
        let obj = r#"{"number": 72, "url": "https://github.com/org/repo/pull/72"}"#;
        let expected = GitPrInfo {
            number: 72,
            url: Some("https://github.com/org/repo/pull/72".into()),
        };
        assert_eq!(parse_gh_pr_json(arr), Some(expected.clone()));
        assert_eq!(parse_gh_pr_json(obj), Some(expected));
        assert_eq!(parse_gh_pr_json("[]"), None);
    }

    #[test]
    fn parse_github_repo_strict_host_and_git_suffix() {
        assert_eq!(
            parse_github_repo("git@github.com:org/repo.git"),
            Some("org/repo".into())
        );
        assert_eq!(
            parse_github_repo("https://github.com/org/repo.git"),
            Some("org/repo".into())
        );
        assert_eq!(
            parse_github_repo("http://GITHUB.COM/org/my-git"),
            Some("org/my-git".into())
        );
        assert_eq!(
            parse_github_repo("ssh://git@github.com/org/repo"),
            Some("org/repo".into())
        );
        assert_eq!(parse_github_repo("https://notgithub.com/org/repo.git"), None);
        assert_eq!(parse_github_repo("https://evil.com/github.com/org/repo"), None);
        assert_eq!(parse_github_repo("git@notgithub.com:org/repo.git"), None);
    }

    #[test]
    fn parse_git_status_porcelain_handles_unmerged_conflicts() {
        let porcelain = "UU conflict1.rs\nAA conflict2.rs\nDD conflict3.rs\nAU conflict4.rs\nM  staged.rs\n M unstaged.rs\n?? new.rs\n";
        let counts = parse_git_status_porcelain(porcelain);
        assert_eq!(
            counts,
            GitStatusCounts {
                staged: 1,
                unstaged: 5,
                untracked: 1,
            }
        );
    }

    #[test]
    fn cached_git_pr_keeps_previous_value_while_refetching() {
        let cwd = PathBuf::from("/rho-test/pr-refetch");
        let pr = GitPrInfo { number: 7, url: None };
        {
            let cache = PR_CACHE.get_or_init(|| Mutex::new(PrCacheEntry::default()));
            *lock(cache) = PrCacheEntry {
                key: Some((cwd.clone(), "feat/pr".to_string())),
                pr: Some(pr.clone()),
                fetched_at: Some(Instant::now() - Duration::from_secs(301)),
                in_flight: false,
            };
        }
        assert_eq!(cached_git_pr(&cwd, Some("feat/pr")), Some(pr));
    }

    #[test]
    fn cached_git_branch_reads_head_and_keeps_gits_last_answer_elsewhere() {
        let repo = rho_test_support::TestDir::new();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/feat/head\n").unwrap();
        assert_eq!(cached_git_branch(&repo), Some("feat/head".to_string()));

        let plain = PathBuf::from("/rho-test/no-repo-branch");
        *lock(&GIT_BRANCH_CACHE) = GitBranchCacheEntry {
            cwd: Some(plain.clone()),
            branch: Some("feat/git".to_string()),
            fetched_at: Some(Instant::now() - GIT_BRANCH_TTL - Duration::from_secs(1)),
            in_flight: false,
        };
        assert_eq!(
            cached_git_branch(&plain),
            Some("feat/git".to_string()),
            "a stale answer stands while git is asked again"
        );
        assert_eq!(cached_git_branch(Path::new("/rho-test/other-no-repo")), None);
    }

    #[test]
    fn a_cached_status_read_does_not_start_git_status() {
        let cwd = PathBuf::from("/rho-test/status-peek");
        *lock(&GIT_STATUS_CACHE) = GitStatusCacheEntry::default();
        assert_eq!(last_git_status_counts(&cwd), None);
        assert_eq!(lock(&GIT_STATUS_CACHE).cwd, None, "nothing was fetched");

        let counts = GitStatusCounts {
            staged: 1,
            unstaged: 2,
            untracked: 3,
        };
        *lock(&GIT_STATUS_CACHE) = GitStatusCacheEntry {
            cwd: Some(cwd.clone()),
            counts: Some(counts),
            fetched_at: Some(Instant::now() - GIT_STATUS_TTL - Duration::from_secs(1)),
            in_flight: false,
        };
        assert_eq!(last_git_status_counts(&cwd), Some(counts));
        assert!(
            lock(&GIT_STATUS_CACHE)
                .fetched_at
                .is_some_and(|t| t.elapsed() > GIT_STATUS_TTL)
        );
    }

    #[test]
    fn cache_entry_is_fresh_respects_hit_and_miss_ttls() {
        let key = (PathBuf::from("/tmp/repo"), "feat/x".to_string());
        let now = Instant::now();
        let hit = PrCacheEntry {
            key: Some(key.clone()),
            pr: Some(GitPrInfo { number: 1, url: None }),
            fetched_at: Some(now - Duration::from_secs(120)),
            in_flight: false,
        };
        assert!(cache_entry_is_fresh(&hit, &key, now));
        let stale_hit = PrCacheEntry {
            fetched_at: Some(now - Duration::from_secs(301)),
            ..hit
        };
        assert!(!cache_entry_is_fresh(&stale_hit, &key, now));

        let miss = PrCacheEntry {
            key: Some(key.clone()),
            pr: None,
            fetched_at: Some(now - Duration::from_secs(30)),
            in_flight: false,
        };
        assert!(cache_entry_is_fresh(&miss, &key, now));
        let stale_miss = PrCacheEntry {
            fetched_at: Some(now - Duration::from_secs(61)),
            ..miss
        };
        assert!(!cache_entry_is_fresh(&stale_miss, &key, now));
    }

    #[test]
    fn run_command_with_timeout_kills_slow_process() {
        let mut ok_cmd = std::process::Command::new("sh");
        ok_cmd.args(["-c", "printf ok"]);
        let out = run_command_with_timeout(ok_cmd, Duration::from_secs(2)).expect("fast command succeeds");
        assert!(out.status.success());
        assert_eq!(out.stdout, b"ok");

        let mut slow_cmd = std::process::Command::new("sh");
        slow_cmd.args(["-c", "sleep 5"]);
        assert!(run_command_with_timeout(slow_cmd, Duration::from_millis(80)).is_none());
    }
}
