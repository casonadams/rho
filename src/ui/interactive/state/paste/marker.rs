use regex::Regex;
use std::sync::LazyLock;

pub(crate) static PASTE_MARKER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[paste #(\d+)(?: (\+\d+ lines|\d+ chars))?\]").expect("valid paste marker regex"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerSpan {
    pub start: usize,
    pub end: usize,
    pub id: usize,
}

pub fn find_marker_ending_at(text: &str, pos: usize) -> Option<MarkerSpan> {
    for cap in PASTE_MARKER_RE.captures_iter(text) {
        let full = cap.get(0)?;
        if full.end() == pos {
            let id = cap.get(1)?.as_str().parse::<usize>().ok()?;
            return Some(MarkerSpan {
                start: full.start(),
                end: full.end(),
                id,
            });
        }
    }
    None
}

pub fn find_marker_starting_at(text: &str, pos: usize) -> Option<MarkerSpan> {
    for cap in PASTE_MARKER_RE.captures_iter(text) {
        let full = cap.get(0)?;
        if full.start() == pos {
            let id = cap.get(1)?.as_str().parse::<usize>().ok()?;
            return Some(MarkerSpan {
                start: full.start(),
                end: full.end(),
                id,
            });
        }
    }
    None
}

pub fn find_marker_covering(text: &str, pos: usize) -> Option<MarkerSpan> {
    for cap in PASTE_MARKER_RE.captures_iter(text) {
        let full = cap.get(0)?;
        if full.start() < pos && pos < full.end() {
            let id = cap.get(1)?.as_str().parse::<usize>().ok()?;
            return Some(MarkerSpan {
                start: full.start(),
                end: full.end(),
                id,
            });
        }
    }
    None
}
