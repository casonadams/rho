mod entry;
mod query;
#[cfg(test)]
mod tests;

pub use entry::{LineMatch, RG_COLLECTION_CEILING, format_results, render};
pub use query::{MAX_RG_FILE_BYTES, RgQuery};
pub use rho_harness_core::args::RgArgs;

use crate::tools::traversal::{CancelOnDrop, DEFAULT_TRAVERSAL_TIMEOUT_SECS, build_type_matcher, search_root};
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub const DEFAULT_RG_LIMIT: usize = 200;
pub const MAX_RG_LIMIT: usize = 1000;

pub struct RgTool {
    base_dir: PathBuf,
}

fn validate_rg_params(
    args: &RgArgs,
    base_dir: &Path,
) -> std::result::Result<(RegexMatcher, Option<ignore::types::Types>, PathBuf), ToolResult> {
    let pattern = args.pattern.trim();
    if pattern.is_empty() {
        return Err(ToolResult::error("Empty pattern provided for rg tool"));
    }
    let matcher = compile_matcher(pattern).map_err(ToolResult::error)?;
    let types = build_type_matcher(args.file_type.as_deref()).map_err(ToolResult::error)?;
    let workspace = Workspace::new(base_dir);
    let search_root = search_root(&workspace, args.path.as_deref()).map_err(ToolResult::error)?;
    Ok((matcher, types, search_root))
}

impl RgTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute(&self, args: RgArgs) -> Result<ToolResult, AppError> {
        let (matcher, types, search_root) = match validate_rg_params(&args, &self.base_dir) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let limit = args.limit.unwrap_or(DEFAULT_RG_LIMIT).clamp(1, MAX_RG_LIMIT);
        let cancellation = Arc::new(AtomicBool::new(false));
        let cancel_guard = CancelOnDrop(cancellation.clone());
        let timeout = Duration::from_secs(DEFAULT_TRAVERSAL_TIMEOUT_SECS);
        let query = build_rg_query(
            (&self.base_dir, args),
            (matcher, types, search_root),
            (timeout, cancellation),
        );
        let handle = tokio::task::spawn_blocking(move || query.run(limit));
        let res = await_rg_task(handle, timeout).await;
        drop(cancel_guard);
        res
    }
}

fn build_rg_query(
    (base_dir, args): (&Path, RgArgs),
    (matcher, types, search_root): (RegexMatcher, Option<ignore::types::Types>, PathBuf),
    (timeout, cancellation): (Duration, Arc<AtomicBool>),
) -> RgQuery {
    RgQuery {
        workspace_root: base_dir.to_path_buf(),
        search_root,
        search_path_display: args.path,
        matcher,
        types,
        include_hidden: args.hidden.unwrap_or(false),
        timeout: Some(timeout),
        cancellation: Some(cancellation),
    }
}

async fn await_rg_task(
    handle: tokio::task::JoinHandle<ToolResult>,
    timeout_dur: Duration,
) -> Result<ToolResult, AppError> {
    match tokio::time::timeout(timeout_dur + Duration::from_secs(1), handle).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => Err(AppError::Tool(format!("rg search task failed: {error}"))),
        Err(_) => Ok(ToolResult::error(format!(
            "Search timed out after {}s",
            timeout_dur.as_secs()
        ))),
    }
}

fn compile_matcher(pattern: &str) -> Result<RegexMatcher, String> {
    let case_insensitive = !pattern.chars().any(char::is_uppercase);
    RegexMatcherBuilder::new()
        .case_insensitive(case_insensitive)
        .build(pattern)
        .map_err(|error| format!("invalid pattern {pattern:?}: {error}"))
}

impl Tool for RgTool {
    const NAME: &'static str = "rg";
    type Args = RgArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Search file contents with a smart-case regex; gitignore-aware, skips binary and large files, bounded."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        generated_schema::<RgArgs>()
    }

    async fn call(&self, _context: &mut ToolContext, args: Self::Args) -> Result<Self::Output, Self::Error> {
        into_rig_result(self.execute(args).await)
    }
}
