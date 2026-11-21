use super::stats::{FileStats, count_file_stats};
use std::cmp::Ordering;
use std::fmt::Write;
use std::path::PathBuf;

use crate::tools::listing::{
    Collected, Layout, Listing, PageRequest, Row, count_noun, directory_order, render_page, split_parent,
};
use crate::tools::truncate::format_size;
use crate::tools::types::ToolResult;
use rho_harness_core::args::FdSort;

pub const FD_COLLECTION_CEILING: usize = 20_000;

const BY_DIRECTORY: Layout = Layout {
    blank_between_groups: false,
    row_indent: "  ",
    preamble: None,
};

const STATS_TABLE: Layout = Layout {
    blank_between_groups: false,
    row_indent: "",
    preamble: Some("  Lines    Bytes  Path"),
};

#[derive(Debug, Clone)]
pub struct FdEntry {
    pub relative: String,
    pub is_dir: bool,
    pub stats: Option<FileStats>,
    pub size: Option<u64>,
    pub source: Option<PathBuf>,
}

impl Row for FdEntry {
    fn group(&self) -> Option<&str> {
        Some(split_parent(&self.relative).0)
    }

    fn render(&self, out: &mut String) {
        out.push_str(split_parent(&self.relative).1);
        if self.is_dir {
            out.push('/');
        }
    }
}

struct StatsRow(FdEntry);

impl Row for StatsRow {
    fn group(&self) -> Option<&str> {
        None
    }

    fn render(&self, out: &mut String) {
        let entry = &self.0;
        let stats = entry
            .stats
            .or_else(|| entry.source.as_deref().and_then(count_file_stats));
        let _ = match stats {
            Some(stats) => write!(
                out,
                "{:>7}  {:>7}  {}",
                stats.lines,
                format_size(stats.bytes as usize),
                entry.relative
            ),
            None => write!(
                out,
                "{:>7}  {:>7}  {}{}",
                "-",
                "-",
                entry.relative,
                if entry.is_dir { "/" } else { "" }
            ),
        };
    }
}

fn entry_bytes(entry: &FdEntry) -> u64 {
    entry.stats.map(|s| s.bytes).or(entry.size).unwrap_or(0)
}

pub fn compare_entries(a: &FdEntry, b: &FdEntry, sort: FdSort) -> Ordering {
    let by_path = || directory_order(&a.relative, &b.relative);
    match sort {
        FdSort::Path => by_path(),
        FdSort::Lines => {
            let lines = |e: &FdEntry| e.stats.map_or(0, |s| s.lines);
            lines(b).cmp(&lines(a)).then_with(by_path)
        }
        FdSort::Size => entry_bytes(b).cmp(&entry_bytes(a)).then_with(by_path),
    }
}

#[derive(Debug, Clone)]
pub struct FdFormat {
    pub request: PageRequest,
    pub show_stats: bool,
    pub artifact_dir: Option<PathBuf>,
}

pub fn format_results(collected: Collected<FdEntry>, options: FdFormat) -> ToolResult {
    if collected.items.is_empty() {
        return ToolResult::success("No paths found");
    }
    let capped = collected.capped();
    let summary = count_noun(collected.seen, "path");
    let artifact_dir = options.artifact_dir.as_deref();
    if options.show_stats {
        let rows: Vec<StatsRow> = collected.items.into_iter().map(StatsRow).collect();
        return render_page(
            &listing(&rows, STATS_TABLE, summary, capped, artifact_dir),
            options.request,
        );
    }
    render_page(
        &listing(&collected.items, BY_DIRECTORY, summary, capped, artifact_dir),
        options.request,
    )
}

fn listing<'a, T>(
    rows: &'a [T],
    layout: Layout,
    summary: String,
    capped: bool,
    artifact_dir: Option<&'a std::path::Path>,
) -> Listing<'a, T> {
    Listing {
        rows,
        layout,
        unit: "path",
        summary,
        capped,
        hint: None,
        cut_notice: None,
        artifact_dir,
    }
}
