use crate::tools::traversal::{build_type_matcher, search_root, walker_builder};
use crate::tools::types::{ToolResult, generated_schema, into_rig_result};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use ignore::WalkState;
use ignore::types::Types;
pub use rho_harness_core::args::RgArgs;
use rho_harness_core::error::AppError;
use rho_harness_core::workspace::Workspace;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::PoisonError;

#[cfg(test)]
mod tests;

pub const DEFAULT_RG_LIMIT: usize = 200;
pub const MAX_RG_LIMIT: usize = 1000;
pub const RG_COLLECTION_CEILING: usize = 5_000;
pub const MAX_RG_LINE_CHARS: usize = 500;
pub const MAX_RG_FILE_BYTES: u64 = 1_000_000;

pub struct RgTool {
    base_dir: PathBuf,
}

impl RgTool {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub async fn execute(&self, args: RgArgs) -> Result<ToolResult, AppError> {
        let pattern = args.pattern.trim();
        if pattern.is_empty() {
            return Ok(ToolResult::error("Empty pattern provided for rg tool"));
        }
        let matcher = match compile_matcher(pattern) {
            Ok(matcher) => matcher,
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
        let limit = args.limit.unwrap_or(DEFAULT_RG_LIMIT).clamp(1, MAX_RG_LIMIT);
        let query = RgQuery {
            workspace_root: workspace.root().to_path_buf(),
            search_root,
            matcher,
            types,
            include_hidden: args.hidden.unwrap_or(false),
        };
        match tokio::task::spawn_blocking(move || query.run(limit)).await {
            Ok(result) => Ok(result),
            Err(error) => Err(AppError::Tool(format!("rg search task failed: {error}"))),
        }
    }
}

fn compile_matcher(pattern: &str) -> Result<RegexMatcher, String> {
    let case_insensitive = !pattern.chars().any(char::is_uppercase);
    RegexMatcherBuilder::new()
        .case_insensitive(case_insensitive)
        .build(pattern)
        .map_err(|error| format!("invalid pattern {pattern:?}: {error}"))
}

struct LineMatch {
    path: String,
    line: u64,
    text: String,
}

struct RgQuery {
    workspace_root: PathBuf,
    search_root: PathBuf,
    matcher: RegexMatcher,
    types: Option<Types>,
    include_hidden: bool,
}

impl RgQuery {
    fn run(self, limit: usize) -> ToolResult {
        let RgQuery {
            workspace_root,
            search_root,
            matcher,
            types,
            include_hidden,
        } = self;
        let mut builder = walker_builder(&search_root, include_hidden);
        if let Some(types) = &types {
            builder.types(types.clone());
        }

        let matches: Mutex<Vec<LineMatch>> = Mutex::new(Vec::new());
        builder.build_parallel().run(|| {
            let mut searcher = SearcherBuilder::new()
                .line_number(true)
                .binary_detection(BinaryDetection::quit(b'\x00'))
                .build();
            // Shared state is captured by reference; the searcher is owned by
            // each visitor so the boxed closure stays self-contained.
            let matches = &matches;
            let matcher = &matcher;
            let workspace_root = &workspace_root;
            Box::new(move |entry| {
                let Ok(entry) = entry else {
                    return WalkState::Continue;
                };
                // The walker's type matcher only filters files, so directory
                // entries still arrive here and must be excluded from search.
                let Some(file_type) = entry.file_type() else {
                    return WalkState::Continue;
                };
                if file_type.is_dir() || file_type.is_symlink() {
                    return WalkState::Continue;
                }
                let Ok(relative) = entry.path().strip_prefix(workspace_root) else {
                    return WalkState::Continue;
                };
                let relative = relative.to_string_lossy().replace('\\', "/");
                if matches.lock().unwrap_or_else(PoisonError::into_inner).len() >= RG_COLLECTION_CEILING {
                    return WalkState::Quit;
                }
                let Ok(metadata) = entry.metadata() else {
                    return WalkState::Continue;
                };
                if metadata.len() > MAX_RG_FILE_BYTES {
                    return WalkState::Continue;
                }
                let mut sink = UTF8(|line_number, line| {
                    let mut matches = matches.lock().unwrap_or_else(PoisonError::into_inner);
                    if matches.len() >= RG_COLLECTION_CEILING {
                        return Ok(false); // stop matching this file; the Quit below follows
                    }
                    matches.push(LineMatch {
                        path: relative.clone(),
                        line: line_number,
                        text: truncate_line(line.trim_end_matches(['\n', '\r'])),
                    });
                    Ok(true)
                });
                // Unreadable files are skipped, never fatal.
                if searcher.search_path(matcher, entry.path(), &mut sink).is_err() {
                    return WalkState::Continue;
                }
                if matches.lock().unwrap_or_else(PoisonError::into_inner).len() >= RG_COLLECTION_CEILING {
                    WalkState::Quit
                } else {
                    WalkState::Continue
                }
            })
        });

        let matches = matches.into_inner().unwrap_or_else(PoisonError::into_inner);
        format_results(matches, limit)
    }
}

fn truncate_line(line: &str) -> String {
    if line.chars().count() <= MAX_RG_LINE_CHARS {
        return line.to_string();
    }
    let truncated: String = line.chars().take(MAX_RG_LINE_CHARS).collect();
    format!("{truncated}\u{2026}")
}

fn render(matches: &[LineMatch]) -> String {
    matches
        .iter()
        .map(|m| format!("{}:{}:{}", m.path, m.line, m.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_results(mut matches: Vec<LineMatch>, limit: usize) -> ToolResult {
    if matches.is_empty() {
        return ToolResult::success("No matches.");
    }
    // Sort before truncating so parallel-walk collection order never leaks into output.
    matches.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    let total = matches.len();
    if total <= limit {
        return ToolResult::success(render(&matches));
    }
    let notice = if total >= RG_COLLECTION_CEILING {
        format!(
            "[showing first {limit} of {RG_COLLECTION_CEILING}+ matches (collection ceiling reached); narrow with a tighter pattern, path, or type]"
        )
    } else {
        format!("[showing first {limit} of {total} matches; narrow with a tighter pattern, path, or type]")
    };
    matches.truncate(limit);
    ToolResult::success(format!("{}\n{notice}", render(&matches)))
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
