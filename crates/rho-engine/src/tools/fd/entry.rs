use super::stats::FileStats;
use std::path::PathBuf;

use crate::tools::truncate::{finish_listing, format_size, listing_limit_notice};
use crate::tools::types::ToolResult;
use rho_harness_core::args::FdSort;

pub const FD_COLLECTION_CEILING: usize = 20_000;

#[derive(Debug, Clone)]
pub struct FdEntry {
    pub relative: String,
    pub is_dir: bool,
    pub stats: Option<FileStats>,
    pub size: Option<u64>,
    pub source: Option<PathBuf>,
}

fn entry_bytes(entry: &FdEntry) -> u64 {
    entry.stats.map(|s| s.bytes).or(entry.size).unwrap_or(0)
}

pub fn sort_entries(entries: &mut [FdEntry], sort: Option<FdSort>) {
    match sort.unwrap_or(FdSort::Path) {
        FdSort::Path => entries.sort_by(|a, b| a.relative.cmp(&b.relative)),
        FdSort::Lines => entries.sort_by(|a, b| {
            let a_lines = a.stats.map_or(0, |s| s.lines);
            let b_lines = b.stats.map_or(0, |s| s.lines);
            b_lines.cmp(&a_lines).then_with(|| a.relative.cmp(&b.relative))
        }),
        FdSort::Size => entries.sort_by(|a, b| {
            let a_bytes = entry_bytes(a);
            let b_bytes = entry_bytes(b);
            b_bytes.cmp(&a_bytes).then_with(|| a.relative.cmp(&b.relative))
        }),
    }
}

#[derive(Debug, Clone)]
pub struct FdFormat {
    pub hit_ceiling: bool,
    pub limit: usize,
    pub show_stats: bool,
    pub artifact_dir: Option<PathBuf>,
}

fn render_fd_content(entries: Vec<FdEntry>, show_stats: bool) -> String {
    if show_stats {
        format_table(&entries)
    } else {
        let mut out = String::with_capacity(entries.len() * 32);
        for (i, entry) in entries.into_iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&display_path(&entry));
        }
        out
    }
}

pub fn format_results(mut entries: Vec<FdEntry>, options: FdFormat) -> ToolResult {
    let FdFormat {
        hit_ceiling,
        limit,
        show_stats,
        artifact_dir,
    } = options;
    if entries.is_empty() {
        return ToolResult::success("No files found matching pattern");
    }
    let limit_notice = (entries.len() > limit)
        .then(|| listing_limit_notice(limit, entries.len(), hit_ceiling.then_some(FD_COLLECTION_CEILING)));
    entries.truncate(limit);
    let content = render_fd_content(entries, show_stats);
    finish_listing(&content, limit_notice, None, artifact_dir.as_deref())
}

fn display_path(entry: &FdEntry) -> String {
    if entry.is_dir {
        format!("{}/", entry.relative)
    } else {
        entry.relative.clone()
    }
}

fn format_table(entries: &[FdEntry]) -> String {
    let mut rows = Vec::with_capacity(entries.len() + 1);
    rows.push(format!("{:>7}  {:>7}  Path", "Lines", "Bytes"));
    for entry in entries {
        match entry.stats {
            Some(stats) => {
                let formatted_size = format_size(stats.bytes as usize);
                rows.push(format!("{:>7}  {:>7}  {}", stats.lines, formatted_size, entry.relative));
            }
            None => {
                rows.push(format!("{:>7}  {:>7}  {}", "-", "-", display_path(entry)));
            }
        }
    }
    rows.join("\n")
}
