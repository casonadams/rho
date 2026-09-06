mod entry;
mod query;
mod stats;
#[cfg(test)]
mod tests;

pub use entry::{FD_COLLECTION_CEILING, FdEntry, FdFormat, format_results, sort_entries};
use query::FdQuery;
pub use rho_harness_core::args::{FdArgs, FdSort};
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::tools::traversal::{CancelOnDrop, DEFAULT_TRAVERSAL_TIMEOUT_SECS, build_type_matcher, search_root};
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use regex::Regex;

pub const DEFAULT_FD_LIMIT: usize = 200;
pub const MAX_FD_LIMIT: usize = 1000;
pub const MAX_FD_DEPTH: usize = 10;

pub struct FdTool {
    base_dir: PathBuf,
}

fn build_fd_regex(pattern: Option<&str>) -> std::result::Result<Option<Regex>, ToolResult> {
    match pattern {
        Some(p) => compile_pattern(p).map(Some).map_err(ToolResult::error),
        None => Ok(None),
    }
}

fn determine_fd_stats(args: &FdArgs) -> (bool, bool) {
    let implied =
        args.min_lines.is_some() || args.max_lines.is_some() || matches!(args.sort, Some(FdSort::Lines | FdSort::Size));
    let show_stats = args.stats.unwrap_or(implied);
    let stats_needed = show_stats || implied;
    (show_stats, stats_needed)
}

fn build_fd_query(
    workspace: &Workspace,
    args: FdArgs,
    (regex, types, search_root): (Option<Regex>, Option<ignore::types::Types>, PathBuf),
) -> FdQuery {
    let (show_stats, stats_needed) = determine_fd_stats(&args);
    FdQuery {
        workspace_root: workspace.root().to_path_buf(),
        search_root,
        search_path_display: args.path,
        regex,
        types,
        include_hidden: args.hidden.unwrap_or(false),
        depth: args.depth.map(|d| d.clamp(1, MAX_FD_DEPTH)),
        stats_needed,
        min_lines: args.min_lines,
        max_lines: args.max_lines,
        sort: args.sort,
        show_stats,
        timeout: None,
        cancellation: None,
    }
}

fn validate_fd_params(
    base_dir: &Path,
    args: &FdArgs,
) -> std::result::Result<(Option<Regex>, Option<ignore::types::Types>, PathBuf), ToolResult> {
    let pattern = args.pattern.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let regex = build_fd_regex(pattern)?;
    let types = build_type_matcher(args.file_type.as_deref()).map_err(ToolResult::error)?;
    let workspace = Workspace::new(base_dir);
    let search_root = search_root(&workspace, args.path.as_deref()).map_err(ToolResult::error)?;
    Ok((regex, types, search_root))
}

async fn await_fd_task(
    handle: tokio::task::JoinHandle<ToolResult>,
    timeout_dur: Duration,
) -> Result<ToolResult, AppError> {
    match tokio::time::timeout(timeout_dur + Duration::from_secs(1), handle).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => Err(AppError::Tool(format!("fd traversal task failed: {error}"))),
        Err(_) => Ok(ToolResult::error(format!(
            "Search timed out after {}s",
            timeout_dur.as_secs()
        ))),
    }
}

impl FdTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute(&self, args: FdArgs) -> Result<ToolResult, AppError> {
        let (regex, types, search_root) = match validate_fd_params(&self.base_dir, &args) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let limit = args.limit.unwrap_or(DEFAULT_FD_LIMIT).clamp(1, MAX_FD_LIMIT);
        let cancellation = Arc::new(AtomicBool::new(false));
        let cancel_guard = CancelOnDrop(cancellation.clone());
        let timeout = Duration::from_secs(DEFAULT_TRAVERSAL_TIMEOUT_SECS);
        let workspace = Workspace::new(&self.base_dir);
        let mut query = build_fd_query(&workspace, args, (regex, types, search_root));
        query.timeout = Some(timeout);
        query.cancellation = Some(cancellation);

        let handle = tokio::task::spawn_blocking(move || query.run(limit));
        let res = await_fd_task(handle, timeout).await;
        drop(cancel_guard);
        res
    }
}

fn compile_pattern(pattern: &str) -> Result<Regex, String> {
    let case_insensitive = !pattern.chars().any(char::is_uppercase);
    regex::RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|error| format!("invalid pattern {pattern:?}: {error}"))
}

impl Tool for FdTool {
    const NAME: &'static str = "fd";
    type Args = FdArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Find files and directories by workspace-relative path with a smart-case regex; gitignore-aware and bounded."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<FdArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
