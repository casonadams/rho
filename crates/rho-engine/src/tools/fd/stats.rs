use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStats {
    pub lines: usize,
    pub bytes: u64,
}

/// Count lines and byte size for a file.
///
/// Follows `str::lines()` line counting semantics: an empty file has 0 lines,
/// and a non-empty file without a trailing newline still counts its final line.
pub fn count_file_stats(path: &Path) -> Option<FileStats> {
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    let bytes = metadata.len();
    if bytes == 0 {
        return Some(FileStats { lines: 0, bytes: 0 });
    }

    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut lines = 0;
    let mut last_byte = None;
    loop {
        let buffer = reader.fill_buf().ok()?;
        if buffer.is_empty() {
            break;
        }
        lines += memchr::memchr_iter(b'\n', buffer).count();
        last_byte = buffer.last().copied();
        let consumed = buffer.len();
        reader.consume(consumed);
    }

    if last_byte.is_some_and(|byte| byte != b'\n') {
        lines += 1;
    }

    Some(FileStats { lines, bytes })
}
