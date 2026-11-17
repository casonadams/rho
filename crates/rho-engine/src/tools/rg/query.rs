use super::entry::{LineMatch, RG_COLLECTION_CEILING, format_results};
use crate::tools::traversal::{run_parallel_walk, walker_builder};
use crate::tools::truncate::truncate_line;
use crate::tools::types::ToolResult;
use grep_regex::RegexMatcher;
use grep_searcher::BinaryDetection;
use grep_searcher::SearcherBuilder;
use grep_searcher::sinks::UTF8;
use ignore::WalkState;
use ignore::types::Types;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

pub const MAX_RG_FILE_BYTES: u64 = 1_000_000;

pub struct RgQuery {
    pub workspace_root: PathBuf,
    pub search_root: PathBuf,
    pub search_path_display: Option<String>,
    pub matcher: RegexMatcher,
    pub types: Option<Types>,
    pub include_hidden: bool,
    pub timeout: Option<Duration>,
    pub cancellation: Option<Arc<AtomicBool>>,
    pub artifact_dir: Option<PathBuf>,
}

fn should_search_entry(entry: &ignore::DirEntry) -> bool {
    let Some(file_type) = entry.file_type() else {
        return false;
    };
    if file_type.is_dir() || file_type.is_symlink() {
        return false;
    }
    entry.metadata().map(|m| m.len() <= MAX_RG_FILE_BYTES).unwrap_or(false)
}

fn search_file(searcher: &mut grep_searcher::Searcher, matcher: &RegexMatcher, path: &Path, ctx: &RgWalkContext<'_>) {
    let mut file_matches = Vec::new();
    let mut sink = UTF8(|line_number, line| {
        let truncated = truncate_line(line.trim_end_matches(['\n', '\r']));
        file_matches.push((line_number, truncated.text.into_owned(), truncated.was_truncated));
        Ok(ctx.match_count.load(Ordering::Relaxed) + file_matches.len() < RG_COLLECTION_CEILING)
    });
    let _ = searcher.search_path(matcher, path, &mut sink);
    if !file_matches.is_empty() {
        let relative = crate::tools::traversal::resolve_relative_path(path, ctx.roots.0, ctx.roots.1, ctx.roots.2);
        let mut list = ctx.matches.lock().unwrap_or_else(PoisonError::into_inner);
        let remaining = RG_COLLECTION_CEILING.saturating_sub(list.len());
        if remaining > 0 {
            file_matches.truncate(remaining);
            for (line_number, text, truncated) in file_matches {
                list.push(LineMatch {
                    path: relative.clone(),
                    line: line_number,
                    text,
                    truncated,
                });
            }
            ctx.match_count.store(list.len(), Ordering::Relaxed);
        }
    }
}

#[derive(Clone, Copy)]
struct RgWalkContext<'a> {
    roots: (&'a Path, &'a Path, Option<&'a str>),
    matches: &'a Mutex<Vec<LineMatch>>,
    match_count: &'a AtomicUsize,
}

fn process_rg_walk_entry(
    entry: Result<ignore::DirEntry, ignore::Error>,
    searcher: &mut grep_searcher::Searcher,
    matcher: &RegexMatcher,
    ctx: &RgWalkContext<'_>,
) -> WalkState {
    let Ok(entry) = entry else { return WalkState::Continue };
    if !should_search_entry(&entry) {
        return WalkState::Continue;
    }
    if ctx.match_count.load(Ordering::Relaxed) >= RG_COLLECTION_CEILING {
        return WalkState::Quit;
    }
    search_file(searcher, matcher, entry.path(), ctx);
    if ctx.match_count.load(Ordering::Relaxed) >= RG_COLLECTION_CEILING {
        WalkState::Quit
    } else {
        WalkState::Continue
    }
}

impl RgQuery {
    pub fn run(self, limit: usize) -> ToolResult {
        let builder = walker_builder(&self.search_root, self.include_hidden, self.types.as_ref());
        let matches = Mutex::new(Vec::new());
        let match_count = AtomicUsize::new(0);
        let ctx = RgWalkContext {
            roots: (
                self.workspace_root.as_path(),
                self.search_root.as_path(),
                self.search_path_display.as_deref(),
            ),
            matches: &matches,
            match_count: &match_count,
        };
        let matcher = &self.matcher;
        let walked = run_parallel_walk(&builder, self.timeout, self.cancellation.as_deref(), || {
            let mut searcher = SearcherBuilder::new()
                .line_number(true)
                .binary_detection(BinaryDetection::quit(b'\x00'))
                .build();
            move |entry| process_rg_walk_entry(entry, &mut searcher, matcher, &ctx)
        });
        if let Err(message) = walked {
            return ToolResult::error(message);
        }
        let list = matches.into_inner().unwrap_or_else(PoisonError::into_inner);
        format_results(list, limit, self.artifact_dir.as_deref())
    }
}
