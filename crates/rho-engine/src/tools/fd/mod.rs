use crate::tools::traversal::{build_type_matcher, search_root, walker_builder};
use crate::tools::truncate::{DEFAULT_MAX_BYTES, format_size, truncate_head};
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use ignore::WalkState;
use ignore::types::Types;
use regex::Regex;
pub use rho_harness_core::args::FdArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
mod tests;

pub const DEFAULT_FD_LIMIT: usize = 200;
pub const MAX_FD_LIMIT: usize = 1000;
pub const FD_COLLECTION_CEILING: usize = 20_000;
pub const MAX_FD_DEPTH: usize = 10;

pub struct FdTool {
    base_dir: PathBuf,
}

impl FdTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute(&self, args: FdArgs) -> Result<ToolResult, AppError> {
        let pattern = args.pattern.trim();
        if pattern.is_empty() {
            return Ok(ToolResult::error("Empty pattern provided for fd tool"));
        }
        let regex = match compile_pattern(pattern) {
            Ok(regex) => regex,
            Err(message) => return Ok(ToolResult::error(message)),
        };
        let types = match build_type_matcher(args.file_type.as_deref()) {
            Ok(types) => types,
            Err(message) => return Ok(ToolResult::error(message)),
        };
        let workspace = Workspace::new(&self.base_dir);
        let search_root = match search_root(&workspace, args.path.as_deref()) {
            Ok(root) => root,
            Err(message) => return Ok(ToolResult::error(message)),
        };
        let limit = args.limit.unwrap_or(DEFAULT_FD_LIMIT).clamp(1, MAX_FD_LIMIT);
        let query = FdQuery {
            workspace_root: workspace.root().to_path_buf(),
            search_root,
            regex,
            types,
            include_hidden: args.hidden.unwrap_or(false),
            depth: args.depth.map(|depth| depth.clamp(1, MAX_FD_DEPTH)),
        };
        match tokio::task::spawn_blocking(move || query.run(limit)).await {
            Ok(result) => Ok(result),
            Err(error) => Err(AppError::Tool(format!("fd traversal task failed: {error}"))),
        }
    }
}

fn compile_pattern(pattern: &str) -> Result<Regex, String> {
    let case_insensitive = !pattern.chars().any(char::is_uppercase);
    regex::RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|error| format!("invalid pattern {pattern:?}: {error}"))
}

struct FdQuery {
    workspace_root: PathBuf,
    search_root: PathBuf,
    regex: Regex,
    types: Option<Types>,
    include_hidden: bool,
    depth: Option<usize>,
}

impl FdQuery {
    fn run(self, limit: usize) -> ToolResult {
        let FdQuery {
            workspace_root,
            search_root,
            regex,
            types,
            include_hidden,
            depth,
        } = self;
        let mut builder = walker_builder(&search_root, include_hidden);
        builder.max_depth(depth);
        if let Some(types) = &types {
            builder.types(types.clone());
        }

        let collected: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let hit_ceiling = AtomicBool::new(false);
        builder.build_parallel().run(|| {
            Box::new(|entry| {
                let Ok(entry) = entry else {
                    return WalkState::Continue;
                };
                let Ok(relative) = entry.path().strip_prefix(&workspace_root) else {
                    return WalkState::Continue;
                };
                let relative = relative.to_string_lossy().replace('\\', "/");
                if relative.is_empty() || !regex.is_match(&relative) {
                    return WalkState::Continue;
                }
                // The walker's type matcher only filters files; type-filtered
                // listings exclude directories while still descending into them.
                if types.is_some() && entry.file_type().is_some_and(|ft| ft.is_dir()) {
                    return WalkState::Continue;
                }
                let mut paths = collected.lock().unwrap_or_else(PoisonError::into_inner);
                if paths.len() >= FD_COLLECTION_CEILING {
                    return WalkState::Quit;
                }
                paths.push(relative);
                if paths.len() >= FD_COLLECTION_CEILING {
                    hit_ceiling.store(true, Ordering::Relaxed);
                    return WalkState::Quit;
                }
                WalkState::Continue
            })
        });

        let paths = collected.into_inner().unwrap_or_else(PoisonError::into_inner);
        format_results(paths, hit_ceiling.load(Ordering::Relaxed), limit)
    }
}

fn format_results(mut paths: Vec<String>, hit_ceiling: bool, limit: usize) -> ToolResult {
    if paths.is_empty() {
        return ToolResult::success("No files found matching pattern");
    }
    // Sort before truncating so parallel-walk collection order never leaks into output.
    paths.sort();
    let total = paths.len();
    let mut notices: Vec<String> = Vec::new();
    if total > limit {
        notices.push(if hit_ceiling {
            format!(
                "showing first {limit} of {FD_COLLECTION_CEILING}+ matches (collection ceiling reached); narrow with a tighter pattern, path, or type"
            )
        } else {
            format!("showing first {limit} of {total} matches; narrow with a tighter pattern, path, or type")
        });
        paths.truncate(limit);
    }
    // pi caps find output by bytes only; the result limit already caps rows.
    let truncation = truncate_head(&paths.join("\n"), usize::MAX, DEFAULT_MAX_BYTES);
    if truncation.truncated_by.is_some() {
        notices.push(format!("{} limit reached", format_size(DEFAULT_MAX_BYTES)));
    }
    let mut output = truncation.content;
    if !notices.is_empty() {
        output.push_str(&format!("\n\n[{}]", notices.join(". ")));
    }
    ToolResult::success(output)
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
