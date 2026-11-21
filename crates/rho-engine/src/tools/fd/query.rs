use super::entry::{FD_COLLECTION_CEILING, FdEntry, FdFormat, compare_entries, format_results};
use super::stats::{FileStats, count_file_stats};
use crate::tools::listing::{PageRequest, SortedCollector};
use crate::tools::traversal::{run_parallel_walk, walker_builder};
use crate::tools::types::ToolResult;
use ignore::WalkState;
use ignore::types::Types;
use regex::Regex;
use rho_harness_core::args::FdSort;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

pub(super) struct FdQuery {
    pub workspace_root: PathBuf,
    pub search_root: PathBuf,
    pub search_path_display: Option<String>,
    pub regex: Option<Regex>,
    pub types: Option<Types>,
    pub include_hidden: bool,
    pub depth: Option<usize>,
    pub stats_needed: bool,
    pub min_lines: Option<usize>,
    pub max_lines: Option<usize>,
    pub sort: Option<FdSort>,
    pub show_stats: bool,
    pub timeout: Option<Duration>,
    pub cancellation: Option<Arc<AtomicBool>>,
    pub artifact_dir: Option<PathBuf>,
}

fn check_stats_lines(path: &Path, min: Option<usize>, max: Option<usize>) -> Option<Option<FileStats>> {
    let s = count_file_stats(path);
    if min.is_some_and(|m| s.map_or(0, |st| st.lines) < m) {
        return None;
    }
    if max.is_some_and(|m| s.map_or(0, |st| st.lines) > m) {
        return None;
    }
    Some(s)
}

struct FdWalkFilters<'a> {
    regex: Option<&'a Regex>,
    types: Option<&'a Types>,
    display: Option<&'a str>,
    min_lines: Option<usize>,
    max_lines: Option<usize>,
    stats_needed: bool,
    size_sort: bool,
    show_stats: bool,
}

fn process_walk_entry(
    entry: ignore::DirEntry,
    w_root: &Path,
    s_root: &Path,
    filters: &FdWalkFilters<'_>,
) -> Option<FdEntry> {
    let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
    let is_search_root = entry.depth() == 0;
    if is_dir
        && (is_search_root || filters.types.is_some() || filters.min_lines.is_some() || filters.max_lines.is_some())
    {
        return None;
    }
    let relative = crate::tools::traversal::resolve_relative_path(entry.path(), w_root, s_root, filters.display);
    if relative.is_empty() || filters.regex.is_some_and(|r| !r.is_match(&relative)) {
        return None;
    }
    let stats = if filters.stats_needed && !is_dir {
        check_stats_lines(entry.path(), filters.min_lines, filters.max_lines)?
    } else {
        None
    };
    let size = (filters.size_sort && !is_dir && stats.is_none())
        .then(|| std::fs::metadata(entry.path()).ok().map(|m| m.len()))
        .flatten();
    let source = (filters.show_stats && !is_dir && stats.is_none()).then(|| entry.path().to_path_buf());
    Some(FdEntry {
        relative,
        is_dir,
        stats,
        size,
        source,
    })
}

impl FdQuery {
    pub fn run(self, request: PageRequest) -> ToolResult {
        let eager_stats =
            self.min_lines.is_some() || self.max_lines.is_some() || matches!(self.sort, Some(FdSort::Lines));
        let mut builder = walker_builder(&self.search_root, self.include_hidden, self.types.as_ref());
        builder.max_depth(self.depth);
        let sort = self.sort.unwrap_or_default();
        let collector = SortedCollector::new(FD_COLLECTION_CEILING, |a: &FdEntry, b: &FdEntry| {
            compare_entries(a, b, sort)
        });
        let seen = AtomicUsize::new(0);
        let filters = FdWalkFilters {
            regex: self.regex.as_ref(),
            types: self.types.as_ref(),
            display: self.search_path_display.as_deref(),
            min_lines: self.min_lines,
            max_lines: self.max_lines,
            stats_needed: self.stats_needed && eager_stats,
            size_sort: matches!(self.sort, Some(FdSort::Size)),
            show_stats: self.show_stats && !eager_stats,
        };
        let (w_root, s_root) = (self.workspace_root.as_path(), self.search_root.as_path());
        let walked = run_parallel_walk(&builder, self.timeout, self.cancellation.as_deref(), || {
            |entry: Result<ignore::DirEntry, ignore::Error>| {
                if let Ok(entry) = entry
                    && let Some(fd_entry) = process_walk_entry(entry, w_root, s_root, &filters)
                {
                    seen.fetch_add(1, Ordering::Relaxed);
                    collector.extend([fd_entry]);
                }
                WalkState::Continue
            }
        });
        if let Err(message) = walked {
            return ToolResult::error(message);
        }
        format_results(
            collector.finish(seen.into_inner()),
            FdFormat {
                request,
                show_stats: self.show_stats,
                artifact_dir: self.artifact_dir,
            },
        )
    }
}
