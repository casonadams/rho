use std::cmp::Ordering;
use std::fmt::Write;

use super::sink::MatchBlock;
use crate::tools::listing::{
    Collected, Layout, Listing, PageRequest, Row, count_noun, directory_order, render_page, split_parent,
};
use crate::tools::truncate::GREP_MAX_LINE_LENGTH;
use crate::tools::types::ToolResult;
use rho_harness_core::args::RgOutputMode;

pub(crate) const RG_COLLECTION_CEILING: usize = 20_000;

const BY_FILE: Layout = Layout {
    blank_between_groups: true,
    row_indent: "",
    preamble: None,
};

#[derive(Debug, Clone)]
pub(crate) struct LineMatch {
    pub path: String,
    pub block: MatchBlock,
}

impl Row for LineMatch {
    fn group(&self) -> Option<&str> {
        Some(&self.path)
    }

    fn render(&self, out: &mut String) {
        if self.block.detached {
            out.push_str("--\n");
        }
        for (i, line) in self.block.lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let _ = write!(out, "{}{}{}", line.number, line.marker, line.text);
        }
    }

    fn cut(&self) -> bool {
        self.block.truncated
    }
}

pub(crate) fn line_order(a: &LineMatch, b: &LineMatch) -> Ordering {
    (&a.path, a.block.first_line()).cmp(&(&b.path, b.block.first_line()))
}

#[derive(Debug, Clone)]
pub(crate) struct FileHit {
    pub path: String,
    pub count: usize,
    pub show_count: bool,
}

impl Row for FileHit {
    fn group(&self) -> Option<&str> {
        Some(split_parent(&self.path).0)
    }

    fn render(&self, out: &mut String) {
        out.push_str(split_parent(&self.path).1);
        if self.show_count {
            let _ = write!(out, ":{}", self.count);
        }
    }
}

pub(crate) fn file_order(a: &FileHit, b: &FileHit) -> Ordering {
    directory_order(&a.path, &b.path)
}

pub(crate) struct RgTotals {
    pub matches: usize,
    pub files: usize,
}

pub(crate) fn format_lines(collected: &Collected<LineMatch>, totals: &RgTotals, request: PageRequest) -> ToolResult {
    if collected.items.is_empty() {
        return ToolResult::success("No matches found");
    }
    let listing = Listing {
        rows: &collected.items,
        layout: BY_FILE,
        unit: "match",
        summary: format!(
            "{} in {}",
            count_noun(totals.matches, "match"),
            count_noun(totals.files, "file")
        ),
        capped: collected.capped(),
        hint: Some("output_mode 'files' or 'count' covers every matching file in far fewer bytes"),
        cut_notice: Some(format!(
            "Some lines truncated to {GREP_MAX_LINE_LENGTH} chars; use read with offset to see full lines"
        )),
    };
    render_page(&listing, request)
}

pub(crate) fn format_files(
    collected: &Collected<FileHit>,
    totals: &RgTotals,
    mode: RgOutputMode,
    request: PageRequest,
) -> ToolResult {
    if collected.items.is_empty() {
        return ToolResult::success("No matches found");
    }
    let files = count_noun(totals.files, "file");
    let summary = match mode {
        RgOutputMode::Count => format!("{} in {files}", count_noun(totals.matches, "match")),
        _ => format!("{files} with matches"),
    };
    let listing = Listing::plain(
        &collected.items,
        Layout::BY_DIRECTORY,
        "file",
        summary,
        collected.capped(),
    );
    render_page(&listing, request)
}
