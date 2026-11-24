//! Automatic `direnv` environment loading for bash commands.
//!
//! Walks up from the command working directory to locate `.envrc` and, when present,
//! invokes `direnv export json` against a clean parent environment (with `DIRENV_*`
//! variables stripped). Never auto-allows `.envrc`: blocked or failing loads degrade
//! silently so commands still execute normally.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tokio::process::Command;

pub const DEFAULT_DIRENV_TIMEOUT_MS: u64 = 30_000;
const ENVRC_CACHE_TTL: Duration = Duration::from_secs(5);
const ENVRC_CACHE_MAX_ENTRIES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirenvOptions {
    pub enabled: bool,
    pub timeout_ms: u64,
}

impl Default for DirenvOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_ms: DEFAULT_DIRENV_TIMEOUT_MS,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirenvDiff {
    pub set: Vec<(String, String)>,
    pub unset: Vec<String>,
}

type EnvrcCache = HashMap<PathBuf, (Instant, Option<PathBuf>)>;
static ENVRC_CACHE: LazyLock<Mutex<EnvrcCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Walks upward from `start_dir` looking for a regular `.envrc` file.
pub fn find_envrc(start_dir: &Path) -> Option<PathBuf> {
    let key = start_dir.to_path_buf();
    let now = Instant::now();
    if let Ok(cache) = ENVRC_CACHE.lock()
        && let Some((at, found)) = cache.get(&key)
        && now.duration_since(*at) < ENVRC_CACHE_TTL
    {
        return found.clone();
    }

    let found = walk_for_envrc(start_dir);
    if let Ok(mut cache) = ENVRC_CACHE.lock() {
        if cache.len() >= ENVRC_CACHE_MAX_ENTRIES {
            cache.clear();
        }
        cache.insert(key, (now, found.clone()));
    }
    found
}

fn walk_for_envrc(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = Some(start_dir);
    while let Some(current) = dir {
        let candidate = current.join(".envrc");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = current.parent();
    }
    None
}

/// Parses `direnv export json` output (`{ "KEY": "val" | null }`).
pub fn parse_direnv_export(stdout: &str) -> Option<DirenvDiff> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Some(DirenvDiff::default());
    }
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(trimmed).ok()?;
    let mut diff = DirenvDiff::default();
    for (key, value) in map {
        match value {
            serde_json::Value::String(val) => diff.set.push((key, val)),
            serde_json::Value::Null => diff.unset.push(key),
            _ => {}
        }
    }
    Some(diff)
}

fn clean_parent_env() -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(k, _)| !k.to_string_lossy().starts_with("DIRENV_"))
        .collect()
}

async fn run_direnv_export(bin: &Path, envrc_dir: &Path, timeout: Duration) -> Option<DirenvDiff> {
    let mut cmd = Command::new(bin);
    cmd.args(["export", "json"])
        .current_dir(envrc_dir)
        .env_clear()
        .envs(clean_parent_env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let child = cmd.spawn().ok()?;
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = std::str::from_utf8(&output.stdout).ok()?;
    parse_direnv_export(stdout)
}

/// Loads the `direnv` environment diff for `cwd` when enabled and `.envrc` exists.
pub async fn load_direnv_env(
    cwd: &Path,
    options: DirenvOptions,
    command_timeout: Option<Duration>,
) -> Option<DirenvDiff> {
    if !options.enabled {
        return None;
    }
    let envrc = find_envrc(cwd)?;
    let envrc_dir = envrc.parent()?;
    let base_timeout = Duration::from_millis(options.timeout_ms.max(1));
    let timeout = command_timeout.map_or(base_timeout, |cmd_t| base_timeout.min(cmd_t));
    run_direnv_export(Path::new("direnv"), envrc_dir, timeout).await
}

/// Applies a loaded `DirenvDiff` onto a child `Command` before spawning.
pub fn apply_direnv(cmd: &mut Command, diff: &DirenvDiff) {
    for key in &diff.unset {
        cmd.env_remove(key);
    }
    for (key, value) in &diff.set {
        cmd.env(key, value);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use rho_test_support::TestDir;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn parse_direnv_export_splits_set_and_unset() {
        let diff = parse_direnv_export(r#"{"FOO":"bar","OLD":null,"NUM":42}"#).unwrap();
        assert_eq!(diff.set, vec![("FOO".to_string(), "bar".to_string())]);
        assert_eq!(diff.unset, vec!["OLD".to_string()]);
        assert_eq!(parse_direnv_export(""), Some(DirenvDiff::default()));
        assert!(parse_direnv_export("not-json").is_none());
    }

    #[test]
    fn find_envrc_walks_ancestors_and_misses_cleanly() {
        let root = TestDir::with_prefix("rho-direnv-walk");
        let nested = root.join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(walk_for_envrc(&nested).is_none());

        let envrc = root.join(".envrc");
        std::fs::write(&envrc, "export HELLO=world\n").unwrap();
        assert_eq!(walk_for_envrc(&nested).as_deref(), Some(envrc.as_path()));
    }

    #[tokio::test]
    async fn run_direnv_export_applies_json_and_ignores_blocked_errors() {
        let dir = TestDir::with_prefix("rho-direnv-exec");
        let ok_bin = dir.join("fake-direnv-ok");
        std::fs::write(
            &ok_bin,
            "#!/bin/sh\nif [ -n \"$DIRENV_DIFF\" ]; then exit 2; fi\nprintf '{\"FROM_DIRENV\":\"ok\",\"DROP_ME\":null}'\n",
        )
        .unwrap();
        std::fs::set_permissions(&ok_bin, std::fs::Permissions::from_mode(0o755)).unwrap();

        let diff = run_direnv_export(&ok_bin, &dir, Duration::from_secs(5)).await.unwrap();
        assert_eq!(diff.set, vec![("FROM_DIRENV".to_string(), "ok".to_string())]);
        assert_eq!(diff.unset, vec!["DROP_ME".to_string()]);

        let blocked_bin = dir.join("fake-direnv-blocked");
        std::fs::write(
            &blocked_bin,
            "#!/bin/sh\necho 'direnv: error .envrc is blocked' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&blocked_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            run_direnv_export(&blocked_bin, &dir, Duration::from_secs(5))
                .await
                .is_none()
        );
    }
}
