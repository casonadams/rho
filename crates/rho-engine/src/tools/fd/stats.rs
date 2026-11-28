use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileStats {
    pub lines: usize,
    pub bytes: u64,
}

/// Counts lines and bytes for a file with `str::lines()` semantics: an empty file has 0 lines, and a final
/// line without a trailing newline still counts.
pub(crate) fn count_file_stats(path: &Path) -> Option<FileStats> {
    let mut file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    let bytes = metadata.len();
    if bytes == 0 {
        return Some(FileStats { lines: 0, bytes: 0 });
    }

    let mut buf = [0u8; 8192];
    let mut lines = 0;
    let mut last_byte = None;
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        let chunk = &buf[..n];
        lines += memchr::memchr_iter(b'\n', chunk).count();
        last_byte = chunk.last().copied();
    }

    if last_byte.is_some_and(|byte| byte != b'\n') {
        lines += 1;
    }

    Some(FileStats { lines, bytes })
}
