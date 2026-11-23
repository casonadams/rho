mod entry;
mod query;
mod sink;
#[cfg(test)]
mod tests;

pub use entry::{FileHit, LineMatch, RG_COLLECTION_CEILING, RgTotals, format_files, format_lines};
pub use query::{MAX_RG_FILE_BYTES, RgQuery};
pub use rho_harness_core::args::{RgArgs, RgOutputMode};
pub use sink::{HitLine, MatchBlock};

pub const MAX_RG_CONTEXT: usize = 10;

use crate::tools::engine_tool::TypedTool;
use crate::tools::listing::PageRequest;
use crate::tools::traversal::{build_type_matcher, is_smart_case_insensitive, run_blocking_search, search_root};
use crate::tools::types::ToolResult;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use std::path::{Path, PathBuf};

pub struct RgTool {
    base_dir: PathBuf,
}

async fn validate_rg_params(
    args: &RgArgs,
    base_dir: &Path,
) -> std::result::Result<(RegexMatcher, Option<ignore::types::Types>, PathBuf), ToolResult> {
    let pattern = args.pattern.trim();
    if pattern.is_empty() {
        return Err(ToolResult::error("Empty pattern provided for rg tool"));
    }
    let multiline = args.multiline.unwrap_or(false);
    let matcher = compile_matcher(pattern, multiline).map_err(ToolResult::error)?;
    let types = build_type_matcher(args.file_type.as_deref()).map_err(ToolResult::error)?;
    let workspace = Workspace::new(base_dir);
    let search_root = search_root(&workspace, args.path.as_deref())
        .await
        .map_err(ToolResult::error)?;
    Ok((matcher, types, search_root))
}

impl RgTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute(&self, args: RgArgs) -> Result<ToolResult, AppError> {
        let (matcher, types, search_root) = match validate_rg_params(&args, &self.base_dir).await {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let request = PageRequest::new(args.offset, args.limit);
        let mut query = RgQuery {
            workspace_root: self.base_dir.clone(),
            search_root,
            search_path_display: args.path,
            matcher,
            types,
            include_hidden: args.hidden.unwrap_or(false),
            mode: args.output_mode.unwrap_or_default(),
            context: args.context.unwrap_or(0).min(MAX_RG_CONTEXT),
            multiline: args.multiline.unwrap_or(false),
            timeout: None,
            cancellation: None,
        };
        run_blocking_search("rg", move |cancellation, timeout| {
            query.timeout = Some(timeout);
            query.cancellation = Some(cancellation);
            query.run(request)
        })
        .await
    }
}

fn compile_matcher(pattern: &str, multiline: bool) -> Result<RegexMatcher, String> {
    RegexMatcherBuilder::new()
        .case_insensitive(is_smart_case_insensitive(pattern))
        .multi_line(multiline)
        .build(pattern)
        .map_err(|error| format!("invalid pattern {pattern:?}: {error}"))
}

#[async_trait::async_trait]
impl TypedTool for RgTool {
    type Args = RgArgs;
    const NAME: &'static str = "rg";
    const CONCURRENCY_SAFE: bool = true;

    async fn run(&self, args: RgArgs) -> Result<ToolResult, AppError> {
        self.execute(args).await
    }
}
