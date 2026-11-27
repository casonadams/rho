use super::entry::{
    FileHit, LineMatch, RG_COLLECTION_CEILING, RgTotals, file_order, format_files, format_lines, line_order,
};
use super::sink::BlockSink;
use crate::locking::lock;
use crate::tools::listing::{PageRequest, SortedCollector};
use crate::tools::traversal::{run_parallel_walk, walker_builder};
use crate::tools::types::ToolResult;
use grep_regex::RegexMatcher;
use grep_searcher::BinaryDetection;
use grep_searcher::SearcherBuilder;
use ignore::WalkState;
use ignore::types::Types;
use rho_harness_core::args::RgOutputMode;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};
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
    pub mode: RgOutputMode,
    pub context: usize,
    pub multiline: bool,
    pub timeout: Option<Duration>,
    pub cancellation: Option<Arc<AtomicBool>>,
}

type Order<T> = fn(&T, &T) -> Ordering;

enum Collector {
    Lines(SortedCollector<LineMatch, Order<LineMatch>>),
    Files(SortedCollector<FileHit, Order<FileHit>>),
}

struct RgWalkContext<'a> {
    roots: (&'a Path, &'a Path, Option<&'a str>),
    mode: RgOutputMode,
    collector: &'a Collector,
    matches: &'a AtomicUsize,
    files: &'a AtomicUsize,
    failures: &'a Mutex<SearchFailures>,
}

#[derive(Default)]
struct SearchFailures {
    count: usize,
    first: Option<String>,
}

impl SearchFailures {
    fn record(&mut self, path: &str, error: &std::io::Error) {
        self.count += 1;
        self.first.get_or_insert_with(|| format!("{path}: {error}"));
    }

    fn note(&self) -> Option<String> {
        let first = self.first.as_ref()?;
        let plural = if self.count == 1 { "" } else { "s" };
        Some(format!(
            "[{} file{plural} could not be searched; first: {first}]",
            self.count
        ))
    }
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
    let mut sink = BlockSink::new(ctx.mode == RgOutputMode::Content, ctx.mode == RgOutputMode::Files);
    let result = searcher.search_path(matcher, path, &mut sink);
    let relative = crate::tools::traversal::resolve_relative_path(path, ctx.roots.0, ctx.roots.1, ctx.roots.2);
    if let Err(error) = result {
        let mut failures = lock(ctx.failures);
        failures.record(&relative, &error);
    }
    let count = sink.count;
    if count == 0 {
        return;
    }
    ctx.matches.fetch_add(count, AtomicOrdering::Relaxed);
    ctx.files.fetch_add(1, AtomicOrdering::Relaxed);
    match ctx.collector {
        Collector::Lines(collector) => collector.extend(sink.blocks.into_iter().map(|block| LineMatch {
            path: relative.clone(),
            block,
        })),
        Collector::Files(collector) => collector.extend([FileHit {
            path: relative,
            count,
            show_count: ctx.mode == RgOutputMode::Count,
        }]),
    }
}

impl RgQuery {
    pub fn run(self, request: PageRequest) -> ToolResult {
        let builder = walker_builder(&self.search_root, self.include_hidden, self.types.as_ref());
        let collector = match self.mode {
            RgOutputMode::Content => Collector::Lines(SortedCollector::new(RG_COLLECTION_CEILING, line_order)),
            _ => Collector::Files(SortedCollector::new(RG_COLLECTION_CEILING, file_order)),
        };
        let (matches, files) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let failures = Mutex::new(SearchFailures::default());
        let ctx = RgWalkContext {
            roots: (
                self.workspace_root.as_path(),
                self.search_root.as_path(),
                self.search_path_display.as_deref(),
            ),
            mode: self.mode,
            collector: &collector,
            matches: &matches,
            files: &files,
            failures: &failures,
        };
        let matcher = &self.matcher;
        let context = if self.mode == RgOutputMode::Content {
            self.context
        } else {
            0
        };
        let walked = run_parallel_walk(&builder, self.timeout, self.cancellation.as_deref(), || {
            let mut searcher = SearcherBuilder::new()
                .line_number(true)
                .before_context(context)
                .after_context(context)
                .multi_line(self.multiline)
                .binary_detection(BinaryDetection::quit(b'\x00'))
                .build();
            let ctx = &ctx;
            move |entry: Result<ignore::DirEntry, ignore::Error>| {
                if let Ok(entry) = entry
                    && should_search_entry(&entry)
                {
                    search_file(&mut searcher, matcher, entry.path(), ctx);
                }
                WalkState::Continue
            }
        });
        if let Err(message) = walked {
            return ToolResult::error(message);
        }
        let totals = RgTotals {
            matches: matches.into_inner(),
            files: files.into_inner(),
        };
        let mut result = match collector {
            Collector::Lines(collector) => format_lines(&collector.finish(totals.matches), &totals, request),
            Collector::Files(collector) => format_files(&collector.finish(totals.files), &totals, self.mode, request),
        };
        let failures = failures.into_inner().unwrap_or_else(PoisonError::into_inner);
        if let Some(note) = failures.note() {
            result.content.push_str("\n\n");
            result.content.push_str(&note);
        }
        result
    }
}
