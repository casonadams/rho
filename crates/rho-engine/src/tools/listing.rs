//! Deterministic collection and byte-budgeted paging for search listings
//! (`rg`, `fd`). Rows are kept in sort order, grouped under a shared header,
//! and every row stays reachable through `offset`.

use std::cmp::Ordering;
use std::sync::{Mutex, PoisonError};

use crate::tools::types::ToolResult;

pub const LISTING_PAGE_BYTES: usize = 20 * 1024;

/// Keeps the first `cap` items in `cmp` order out of everything pushed, so the
/// kept set does not depend on parallel walk order.
pub struct SortedCollector<T, F> {
    cap: usize,
    cmp: F,
    items: Mutex<Vec<T>>,
}

pub struct Collected<T> {
    pub items: Vec<T>,
    pub seen: usize,
}

impl<T> Collected<T> {
    pub fn capped(&self) -> bool {
        self.seen > self.items.len()
    }
}

impl<T, F: Fn(&T, &T) -> Ordering> SortedCollector<T, F> {
    pub fn new(cap: usize, cmp: F) -> Self {
        Self {
            cap: cap.max(1),
            cmp,
            items: Mutex::new(Vec::new()),
        }
    }

    pub fn extend(&self, batch: impl IntoIterator<Item = T>) {
        let mut items = self.items.lock().unwrap_or_else(PoisonError::into_inner);
        items.extend(batch);
        if items.len() > self.cap * 2 {
            items.sort_by(&self.cmp);
            items.truncate(self.cap);
        }
    }

    pub fn finish(self, seen: usize) -> Collected<T> {
        let mut items = self.items.into_inner().unwrap_or_else(PoisonError::into_inner);
        items.sort_by(&self.cmp);
        items.truncate(self.cap);
        Collected { items, seen }
    }
}

pub trait Row {
    fn group(&self) -> Option<&str>;
    fn render(&self, out: &mut String);
    fn cut(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub blank_between_groups: bool,
    pub row_indent: &'static str,
    pub preamble: Option<&'static str>,
}

impl Layout {
    /// Rows indented under a header line naming their directory.
    pub const BY_DIRECTORY: Layout = Layout {
        blank_between_groups: false,
        row_indent: "  ",
        preamble: None,
    };
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PageRequest {
    pub offset: usize,
    pub limit: Option<usize>,
}

impl PageRequest {
    pub fn new(offset: Option<usize>, limit: Option<usize>) -> Self {
        Self {
            offset: offset.unwrap_or(0),
            limit,
        }
    }
}

pub struct Listing<'a, T> {
    pub rows: &'a [T],
    pub layout: Layout,
    pub unit: &'static str,
    pub summary: String,
    pub capped: bool,
    pub hint: Option<&'static str>,
    pub cut_notice: Option<String>,
}

impl<'a, T> Listing<'a, T> {
    /// A listing with no hint and no cut-line notice.
    pub fn plain(rows: &'a [T], layout: Layout, unit: &'static str, summary: String, capped: bool) -> Self {
        Self {
            rows,
            layout,
            unit,
            summary,
            capped,
            hint: None,
            cut_notice: None,
        }
    }
}

struct Page {
    body: String,
    shown: usize,
    any_cut: bool,
}

fn render_rows<T: Row>(rows: &[T], layout: Layout, limit: usize, budget: usize) -> Page {
    let mut body = String::new();
    if let Some(preamble) = layout.preamble {
        body.push_str(preamble);
        body.push('\n');
    }
    let mut current: Option<&str> = None;
    let (mut shown, mut any_cut) = (0, false);
    let mut chunk = String::new();
    for row in rows.iter().take(limit) {
        chunk.clear();
        let group = row.group();
        if shown == 0 || group != current {
            if shown > 0 && layout.blank_between_groups {
                chunk.push('\n');
            }
            if let Some(header) = group {
                chunk.push_str(header);
                chunk.push('\n');
            }
        }
        chunk.push_str(layout.row_indent);
        row.render(&mut chunk);
        chunk.push('\n');
        if shown > 0 && body.len() + chunk.len() > budget {
            break;
        }
        body.push_str(&chunk);
        current = group;
        shown += 1;
        any_cut |= row.cut();
    }
    body.truncate(body.trim_end_matches('\n').len());
    Page { body, shown, any_cut }
}

fn plural(unit: &str) -> String {
    match unit {
        "match" => "matches".into(),
        "entry" => "entries".into(),
        _ => format!("{unit}s"),
    }
}

pub fn count_noun(count: usize, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit}")
    } else {
        format!("{count} {}", plural(unit))
    }
}

fn paging_notice<T>(listing: &Listing<'_, T>, start: usize, end: usize) -> String {
    let total = listing.rows.len();
    let units = plural(listing.unit);
    let mut parts = vec![format!("showing {units} {}-{end} of {total}", start + 1)];
    if end < total {
        parts.push(format!("next page: offset={end}"));
    } else if listing.capped {
        parts.push(format!(
            "only the first {total} {units} in sort order can be paged; narrow with path, type, or pattern to reach the rest"
        ));
    }
    if start == 0
        && let Some(hint) = listing.hint
    {
        parts.push(hint.to_string());
    }
    parts.join("; ")
}

pub fn render_page<T: Row>(listing: &Listing<'_, T>, request: PageRequest) -> ToolResult {
    let total = listing.rows.len();
    let start = request.offset;
    if start >= total {
        return ToolResult::success(format!(
            "{}\n\n[offset={start} is past the end; valid offsets are 0-{}]",
            listing.summary,
            total.saturating_sub(1)
        ));
    }
    let limit = request.limit.unwrap_or(usize::MAX).max(1);
    let page = render_rows(&listing.rows[start..], listing.layout, limit, LISTING_PAGE_BYTES);
    let end = start + page.shown;
    let mut output = format!("{}\n\n{}", listing.summary, page.body);
    let complete = start == 0 && end == total && !listing.capped;
    let mut notices = Vec::new();
    if !complete {
        notices.push(paging_notice(listing, start, end));
    }
    if page.any_cut
        && let Some(cut) = &listing.cut_notice
    {
        notices.push(cut.clone());
    }
    if !notices.is_empty() {
        output.push_str(&format!("\n\n[{}]", notices.join(". ")));
    }
    ToolResult::success(output)
}

pub fn split_parent(path: &str) -> (&str, &str) {
    match parent_key(path) {
        ("", name) => ("./", name),
        split => split,
    }
}

fn parent_key(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(idx) => (&path[..=idx], &path[idx + 1..]),
        None => ("", path),
    }
}

/// Orders paths so each directory's direct children are contiguous, which
/// keeps one header per directory in a grouped listing.
pub fn directory_order(a: &str, b: &str) -> Ordering {
    parent_key(a).cmp(&parent_key(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Item(&'static str, &'static str);

    impl Row for Item {
        fn group(&self) -> Option<&str> {
            Some(self.0)
        }
        fn render(&self, out: &mut String) {
            out.push_str(self.1);
        }
    }

    const DIRS: Layout = Layout {
        blank_between_groups: false,
        row_indent: "  ",
        preamble: None,
    };

    fn listing(rows: &[Item]) -> Listing<'_, Item> {
        Listing {
            rows,
            layout: DIRS,
            unit: "file",
            summary: count_noun(rows.len(), "file"),
            capped: false,
            hint: None,
            cut_notice: None,
        }
    }

    #[test]
    fn collector_keeps_the_smallest_items_regardless_of_push_order() {
        let forward = SortedCollector::new(3, |a: &u32, b: &u32| a.cmp(b));
        let backward = SortedCollector::new(3, |a: &u32, b: &u32| a.cmp(b));
        for i in 0..50 {
            forward.extend([i]);
            backward.extend([49 - i]);
        }
        let (forward, backward) = (forward.finish(50), backward.finish(50));
        assert_eq!(forward.items, vec![0, 1, 2]);
        assert_eq!(backward.items, forward.items);
        assert!(forward.capped());
    }

    #[test]
    fn complete_listing_groups_rows_without_notices() {
        let rows = [Item("src/", "a.rs"), Item("src/", "b.rs"), Item("src/ui/", "c.rs")];
        let result = render_page(&listing(&rows), PageRequest::default());
        assert_eq!(result.content, "3 files\n\nsrc/\n  a.rs\n  b.rs\nsrc/ui/\n  c.rs");
    }

    #[test]
    fn limit_pages_with_the_exact_next_offset_and_repeats_the_group_header() {
        let rows = [Item("src/", "a.rs"), Item("src/", "b.rs"), Item("src/", "c.rs")];
        let first = render_page(
            &listing(&rows),
            PageRequest {
                offset: 0,
                limit: Some(2),
            },
        );
        assert_eq!(
            first.content,
            "3 files\n\nsrc/\n  a.rs\n  b.rs\n\n[showing files 1-2 of 3; next page: offset=2]"
        );
        let second = render_page(
            &listing(&rows),
            PageRequest {
                offset: 2,
                limit: Some(2),
            },
        );
        assert_eq!(second.content, "3 files\n\nsrc/\n  c.rs\n\n[showing files 3-3 of 3]");
    }

    #[test]
    fn offset_past_the_end_names_the_valid_range() {
        let rows = [Item("src/", "a.rs")];
        let result = render_page(&listing(&rows), PageRequest { offset: 5, limit: None });
        assert_eq!(
            result.content,
            "1 file\n\n[offset=5 is past the end; valid offsets are 0-0]"
        );
    }

    #[test]
    fn byte_budget_ends_the_page_on_a_whole_row() {
        let name: &'static str = Box::leak("n".repeat(1000).into_boxed_str());
        let rows: Vec<Item> = (0..100).map(|_| Item("d/", name)).collect();
        let result = render_page(&listing(&rows), PageRequest::default());
        let body = result.content.split("\n\n").nth(1).unwrap();
        assert!(body.len() <= LISTING_PAGE_BYTES);
        let shown = body.lines().count() - 1;
        assert!(
            result
                .content
                .ends_with(&format!("[showing files 1-{shown} of 100; next page: offset={shown}]"))
        );
    }

    #[test]
    fn capped_listing_explains_how_to_reach_unkept_rows() {
        let rows = [Item("src/", "a.rs")];
        let mut paged = listing(&rows);
        paged.capped = true;
        let result = render_page(&paged, PageRequest::default());
        assert!(result.content.ends_with(
            "[showing files 1-1 of 1; only the first 1 files in sort order can be paged; narrow with path, type, or pattern to reach the rest]"
        ));
    }

    #[test]
    fn split_parent_uses_dot_slash_at_the_root() {
        assert_eq!(split_parent("README.md"), ("./", "README.md"));
        assert_eq!(split_parent("src/ui/widget.rs"), ("src/ui/", "widget.rs"));
    }

    #[test]
    fn directory_order_keeps_siblings_together_with_the_root_first() {
        let mut paths = vec![
            "src/ui/a.rs",
            "build.rs",
            "src/ui",
            "-dash/x",
            "assets/i.ico",
            "README.md",
            "src/b.rs",
        ];
        paths.sort_by(|a, b| directory_order(a, b));
        assert_eq!(
            paths,
            [
                "README.md",
                "build.rs",
                "-dash/x",
                "assets/i.ico",
                "src/b.rs",
                "src/ui",
                "src/ui/a.rs"
            ]
        );
    }
}
