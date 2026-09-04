use super::stats::FileStats;
use crate::tools::truncate::{DEFAULT_MAX_BYTES, format_size, truncate_head};
use crate::tools::types::ToolResult;
use rho_harness_core::args::FdSort;

pub const FD_COLLECTION_CEILING: usize = 20_000;

#[derive(Debug, Clone)]
pub struct FdEntry {
    pub relative: String,
    pub is_dir: bool,
    pub stats: Option<FileStats>,
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
            let a_bytes = a.stats.map_or(0, |s| s.bytes);
            let b_bytes = b.stats.map_or(0, |s| s.bytes);
            b_bytes.cmp(&a_bytes).then_with(|| a.relative.cmp(&b.relative))
        }),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FdFormat {
    pub hit_ceiling: bool,
    pub limit: usize,
    pub show_stats: bool,
}

pub fn format_results(mut entries: Vec<FdEntry>, options: FdFormat) -> ToolResult {
    let FdFormat {
        hit_ceiling,
        limit,
        show_stats,
    } = options;
    if entries.is_empty() {
        return ToolResult::success("No files found matching pattern");
    }
    let total = entries.len();
    let mut notices: Vec<String> = Vec::new();
    if total > limit {
        notices.push(if hit_ceiling {
            format!(
                "showing first {limit} of {FD_COLLECTION_CEILING}+ matches (collection ceiling reached); narrow with a tighter pattern, path, or type"
            )
        } else {
            format!("showing first {limit} of {total} matches; narrow with a tighter pattern, path, or type")
        });
        entries.truncate(limit);
    }

    let content = if show_stats {
        format_table(&entries)
    } else {
        entries
            .into_iter()
            .map(|entry| entry.relative)
            .collect::<Vec<_>>()
            .join("\n")
    };

    let truncation = truncate_head(&content, usize::MAX, DEFAULT_MAX_BYTES);
    if truncation.truncated_by.is_some() {
        notices.push(format!("{} limit reached", format_size(DEFAULT_MAX_BYTES)));
    }
    let mut output = truncation.content;
    if !notices.is_empty() {
        output.push_str(&format!("\n\n[{}]", notices.join(". ")));
    }
    ToolResult::success(output)
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
                rows.push(format!("{:>7}  {:>7}  {}", "-", "-", entry.relative));
            }
        }
    }
    rows.join("\n")
}
