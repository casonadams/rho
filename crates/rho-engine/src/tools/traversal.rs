//! Shared filesystem traversal configuration for the fd and rg tools.

use crate::tools::types::ToolResult;
use ignore::types::{Types, TypesBuilder};
use ignore::{DirEntry, WalkBuilder, WalkState};
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const DEFAULT_TRAVERSAL_TIMEOUT_SECS: u64 = 30;

struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Runs `job` on a blocking thread with the default timeout, and stops it if this future is dropped.
pub async fn run_blocking_search(
    label: &str,
    job: impl FnOnce(Arc<AtomicBool>, Duration) -> ToolResult + Send + 'static,
) -> Result<ToolResult, AppError> {
    let cancellation = Arc::new(AtomicBool::new(false));
    let _stop_on_drop = CancelOnDrop(cancellation.clone());
    let timeout = Duration::from_secs(DEFAULT_TRAVERSAL_TIMEOUT_SECS);
    let handle = tokio::task::spawn_blocking(move || job(cancellation, timeout));
    match tokio::time::timeout(timeout + Duration::from_secs(1), handle).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => Err(AppError::Tool(format!("{label} task failed: {error}"))),
        Err(_) => Ok(ToolResult::error(format!(
            "Search timed out after {}s",
            timeout.as_secs()
        ))),
    }
}

/// A pattern with no uppercase letter matches case-insensitively.
pub fn is_smart_case_insensitive(pattern: &str) -> bool {
    !pattern.chars().any(char::is_uppercase)
}

fn should_quit_traversal(
    cancellation: Option<&AtomicBool>,
    timed_out: &AtomicBool,
    start: Instant,
    timeout: Duration,
) -> bool {
    if let Some(c) = cancellation
        && c.load(Ordering::Relaxed)
    {
        return true;
    }
    if timed_out.load(Ordering::Relaxed) {
        return true;
    }
    if start.elapsed() >= timeout {
        timed_out.store(true, Ordering::Relaxed);
        return true;
    }
    false
}

/// Runs `builder` across threads, giving each thread a visitor from
/// `make_visitor`, until a visitor quits, `cancellation` fires, or `timeout`
/// (default `DEFAULT_TRAVERSAL_TIMEOUT_SECS`) elapses.
pub fn run_parallel_walk<'s, V>(
    builder: &WalkBuilder,
    timeout: Option<Duration>,
    cancellation: Option<&'s AtomicBool>,
    mut make_visitor: impl FnMut() -> V,
) -> Result<(), String>
where
    V: FnMut(Result<DirEntry, ignore::Error>) -> WalkState + Send + 's,
{
    let timeout = timeout.unwrap_or(Duration::from_secs(DEFAULT_TRAVERSAL_TIMEOUT_SECS));
    let start = Instant::now();
    let timed_out = AtomicBool::new(false);
    builder.build_parallel().run(|| {
        let mut visit = make_visitor();
        let timed_out = &timed_out;
        Box::new(move |entry| {
            if should_quit_traversal(cancellation, timed_out, start, timeout) {
                return WalkState::Quit;
            }
            visit(entry)
        })
    });
    if timed_out.into_inner() {
        return Err(format!("Search timed out after {}s", timeout.as_secs()));
    }
    Ok(())
}

/// Builds a workspace-scoped walker: ignore rules (.gitignore, .ignore, the
/// global gitignore, .git/info/exclude) and hidden entries are respected
/// unless `include_hidden`, and symlinks are never followed.
pub fn walker_builder(search_root: &Path, include_hidden: bool, types: Option<&Types>) -> WalkBuilder {
    let mut builder = WalkBuilder::new(search_root);
    if let Some(types) = types {
        builder.types(types.clone());
    }
    builder
        .hidden(!include_hidden)
        .follow_links(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(true);
    builder
}

/// Lists up to `max_files` files under `root` as sorted, `/`-separated
/// relative paths, honoring the same ignore and hidden rules as fd and rg.
pub fn list_workspace_files(root: &Path, max_files: usize) -> Vec<String> {
    let mut files: Vec<String> = walker_builder(root, false, None)
        .build()
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|file_type| file_type.is_file()))
        .filter_map(|entry| {
            let relative = entry.path().strip_prefix(root).ok()?;
            Some(relative.to_string_lossy().replace('\\', "/"))
        })
        .take(max_files)
        .collect();
    files.sort();
    files
}

/// Selects a default file-type definition (e.g. 'rust', 'py'); unknown names
/// are rejected with the existing fd/rg error phrasing.
pub fn build_type_matcher(file_type: Option<&str>) -> Result<Option<Types>, String> {
    let Some(file_type) = file_type.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let mut builder = TypesBuilder::new();
    builder.add_defaults().select(file_type);
    builder
        .build()
        .map(Some)
        .map_err(|_| format!("unknown type {file_type:?}; use a default type name such as 'rust', 'js', or 'py'"))
}

/// Resolves the optional `path` argument to an absolute search root,
/// rejecting paths that do not exist.
pub async fn search_root(workspace: &Workspace, path: Option<&str>) -> Result<PathBuf, String> {
    let Some(raw) = path.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(workspace.root().to_path_buf());
    };
    match workspace.resolve(raw) {
        Some(root) if root.exists() => Ok(root),
        _ => Err(format!(
            "path not found: {raw}{}",
            super::suggest::missing_path_hint(workspace.root(), raw, super::suggest::PathKind::Any).await
        )),
    }
}

pub fn resolve_relative_path(
    path: &Path,
    workspace_root: &Path,
    search_root: &Path,
    search_path_display: Option<&str>,
) -> String {
    if let Ok(rel) = path.strip_prefix(workspace_root) {
        rel.to_string_lossy().replace('\\', "/")
    } else if let Ok(rel) = path.strip_prefix(search_root) {
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let base = search_path_display.unwrap_or("");
        if rel_str.is_empty() {
            base.to_string()
        } else if base.is_empty() || base.ends_with('/') {
            format!("{base}{rel_str}")
        } else {
            format!("{base}/{rel_str}")
        }
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

#[cfg(test)]
mod tests {
    use super::{is_smart_case_insensitive, list_workspace_files, run_blocking_search};
    use crate::tools::types::ToolResult;
    use std::fs;

    fn workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::create_dir_all(dir.path().join("ignored")).unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
        fs::write(dir.path().join(".hidden"), "").unwrap();
        fs::write(dir.path().join("b.txt"), "").unwrap();
        fs::write(dir.path().join("sub/a.rs"), "").unwrap();
        fs::write(dir.path().join("ignored/c.txt"), "").unwrap();
        dir
    }

    #[test]
    fn list_workspace_files_skips_ignored_and_hidden_entries() {
        let dir = workspace();
        assert_eq!(list_workspace_files(dir.path(), 100), ["b.txt", "sub/a.rs"]);
    }

    #[test]
    fn list_workspace_files_stops_at_max_files() {
        let dir = workspace();
        assert_eq!(list_workspace_files(dir.path(), 1).len(), 1);
    }

    #[test]
    fn patterns_without_uppercase_match_case_insensitively() {
        let cases = [
            ("todo", true),
            ("a.b", true),
            ("TODO", false),
            ("toDo", false),
            ("", true),
        ];
        for (pattern, insensitive) in cases {
            assert_eq!(is_smart_case_insensitive(pattern), insensitive, "{pattern:?}");
        }
    }

    #[tokio::test]
    async fn blocking_search_returns_the_job_result_and_reports_a_panicked_job() {
        let done = run_blocking_search("fd", |_, timeout| {
            ToolResult::success(format!("ran with {}s", timeout.as_secs()))
        })
        .await
        .unwrap();
        assert_eq!(done.content, "ran with 30s");

        let failed = run_blocking_search("fd", |_, _| panic!("boom")).await.unwrap_err();
        assert!(failed.to_string().contains("fd task failed"), "{failed}");
    }
}
