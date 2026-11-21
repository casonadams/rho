use std::cmp::Ordering;
use std::fmt::Write;
use std::path::Path;

use crate::tools::listing::{
    Collected, Layout, Listing, PageRequest, Row, count_noun, directory_order, render_page, split_parent,
};
use crate::tools::truncate::GREP_MAX_LINE_LENGTH;
use crate::tools::types::ToolResult;
use rho_harness_core::args::RgOutputMode;

pub const RG_COLLECTION_CEILING: usize = 20_000;

const BY_FILE: Layout = Layout {
    blank_between_groups: true,
    row_indent: "",
    preamble: None,
};

const BY_DIRECTORY: Layout = Layout {
    blank_between_groups: false,
    row_indent: "  ",
    preamble: None,
};

#[derive(Debug, Clone)]
pub struct LineMatch {
    pub path: String,
    pub line: u64,
    pub text: String,
    pub truncated: bool,
}

impl Row for LineMatch {
    fn group(&self) -> Option<&str> {
        Some(&self.path)
    }

    fn render(&self, out: &mut String) {
        let _ = write!(out, "{}:{}", self.line, self.text);
    }

    fn cut(&self) -> bool {
        self.truncated
    }
}

pub fn line_order(a: &LineMatch, b: &LineMatch) -> Ordering {
    (&a.path, a.line).cmp(&(&b.path, b.line))
}

#[derive(Debug, Clone)]
pub struct FileHit {
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

pub fn file_order(a: &FileHit, b: &FileHit) -> Ordering {
    directory_order(&a.path, &b.path)
}

pub struct RgTotals {
    pub matches: usize,
    pub files: usize,
}

pub fn format_lines(
    collected: &Collected<LineMatch>,
    totals: &RgTotals,
    request: PageRequest,
    artifact_dir: Option<&Path>,
) -> ToolResult {
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
        artifact_dir,
    };
    render_page(&listing, request)
}

pub fn format_files(
    collected: &Collected<FileHit>,
    totals: &RgTotals,
    mode: RgOutputMode,
    request: PageRequest,
    artifact_dir: Option<&Path>,
) -> ToolResult {
    if collected.items.is_empty() {
        return ToolResult::success("No matches found");
    }
    let files = count_noun(totals.files, "file");
    let summary = match mode {
        RgOutputMode::Count => format!("{} in {files}", count_noun(totals.matches, "match")),
        _ => format!("{files} with matches"),
    };
    let listing = Listing {
        rows: &collected.items,
        layout: BY_DIRECTORY,
        unit: "file",
        summary,
        capped: collected.capped(),
        hint: None,
        cut_notice: None,
        artifact_dir,
    };
    render_page(&listing, request)
}
