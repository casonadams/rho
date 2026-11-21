mod entry;
mod query;
mod stats;
#[cfg(test)]
mod tests;

pub use entry::{FD_COLLECTION_CEILING, FdEntry, FdFormat, compare_entries, format_results};
use query::FdQuery;
pub use rho_harness_core::args::{FdArgs, FdSort};
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use std::path::{Path, PathBuf};

use crate::tools::engine_tool::EngineTool;
use crate::tools::listing::PageRequest;
use crate::tools::traversal::{build_type_matcher, is_smart_case_insensitive, run_blocking_search, search_root};
use crate::tools::types::{ToolResult, generated_schema};
use regex::Regex;

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

async fn validate_fd_params(
    base_dir: &Path,
    args: &FdArgs,
) -> std::result::Result<(Option<Regex>, Option<ignore::types::Types>, PathBuf), ToolResult> {
    let pattern = args.pattern.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let regex = build_fd_regex(pattern)?;
    let types = build_type_matcher(args.file_type.as_deref()).map_err(ToolResult::error)?;
    let workspace = Workspace::new(base_dir);
    let search_root = search_root(&workspace, args.path.as_deref())
        .await
        .map_err(ToolResult::error)?;
    Ok((regex, types, search_root))
}

impl FdTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute(&self, args: FdArgs) -> Result<ToolResult, AppError> {
        let (regex, types, search_root) = match validate_fd_params(&self.base_dir, &args).await {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let request = PageRequest::new(args.offset, args.limit);
        let workspace = Workspace::new(&self.base_dir);
        let mut query = build_fd_query(&workspace, args, (regex, types, search_root));
        run_blocking_search("fd", move |cancellation, timeout| {
            query.timeout = Some(timeout);
            query.cancellation = Some(cancellation);
            query.run(request)
        })
        .await
    }
}

fn compile_pattern(pattern: &str) -> Result<Regex, String> {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(is_smart_case_insensitive(pattern))
        .build()
        .map_err(|error| format!("invalid pattern {pattern:?}: {error}"))
}

#[async_trait::async_trait]
impl EngineTool for FdTool {
    fn name(&self) -> &str {
        "fd"
    }

    fn description(&self) -> &str {
        crate::tools::ToolRegistry::builtin_description("fd")
    }

    fn concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<FdArgs>()
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, AppError> {
        let args: FdArgs = match crate::tools::types::parse_tool_args("fd", args, || self.parameters()) {
            Ok(a) => a,
            Err(e) => return Ok(e),
        };
        self.execute(args).await
    }
}
