use regex::Regex;
use std::sync::LazyLock;

pub(crate) static PASTE_MARKER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[paste #(\d+)(?: (\+\d+ lines|\d+ chars))?\]").expect("valid paste marker regex"));
