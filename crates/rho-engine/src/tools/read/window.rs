//! Reads the requested lines of a text file too large to load whole, streaming past the rest.

use std::io::BufRead;

use super::format::number_lines;
use crate::tools::truncate::{DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, format_size};
use crate::tools::types::ToolResult;

/// The 1-based first line and the line count a read asks for.
#[derive(Clone, Copy)]
pub(crate) struct LineWindow {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

enum Collected {
    /// Lines that fit the output, and whether more lines follow them.
    Lines { lines: Vec<Vec<u8>>, more: bool },
    /// The start of a first line too long for the output on its own.
    Oversized(Vec<u8>),
}

/// The lines `window` asks for from `reader`, a text file of `file_size` bytes.
pub(crate) fn read_window(
    mut reader: impl BufRead,
    file_size: u64,
    window: LineWindow,
    clean_path: &str,
) -> std::io::Result<ToolResult> {
    let offset = window.offset.unwrap_or(1).max(1);
    let skipped = skip_lines(&mut reader, offset - 1)?;
    if reader.fill_buf()?.is_empty() {
        return Ok(ToolResult::error(format!(
            "Offset {offset} is beyond end of file ({skipped} lines total)"
        )));
    }
    let max_lines = window.limit.unwrap_or(usize::MAX).min(DEFAULT_MAX_LINES);
    let (lines, more) = match collect_lines(&mut reader, max_lines)? {
        Collected::Lines { lines, more } => (lines, more),
        Collected::Oversized(head) => return oversized_line(reader, head, offset, clean_path),
    };
    let Ok(text) = String::from_utf8(lines.join(&b'\n')) else {
        return Ok(invalid_utf8(clean_path));
    };
    let mut output = number_lines(&text, offset);
    let end = offset + lines.len() - 1;
    let next = end + 1;
    let user_limit_reached = window.limit == Some(lines.len());
    if more && user_limit_reached {
        output.push_str(&format!("\n\n[More lines in file. Use offset={next} to continue.]"));
    } else if more {
        output.push_str(&format!(
            "\n\n[Showing lines {offset}-{end} of a {} file. Use offset={next} to continue.]",
            format_size(file_size as usize)
        ));
    }
    Ok(ToolResult::success(output))
}

/// Skips up to `count` lines and returns how many there were.
fn skip_lines(reader: &mut impl BufRead, count: usize) -> std::io::Result<usize> {
    for skipped in 0..count {
        if reader.skip_until(b'\n')? == 0 {
            return Ok(skipped);
        }
    }
    Ok(count)
}

/// Reads up to `max_lines` whole lines, without their line endings, within the output budget.
fn collect_lines(reader: &mut impl BufRead, max_lines: usize) -> std::io::Result<Collected> {
    let mut lines = Vec::new();
    let mut budget = DEFAULT_MAX_BYTES;
    while lines.len() < max_lines && !reader.fill_buf()?.is_empty() {
        let mut line = Vec::new();
        std::io::Read::take(reader.by_ref(), budget as u64).read_until(b'\n', &mut line)?;
        let complete = line.ends_with(b"\n") || reader.fill_buf()?.is_empty();
        if !complete && lines.is_empty() {
            return Ok(Collected::Oversized(line));
        }
        if !complete {
            return Ok(Collected::Lines { lines, more: true });
        }
        budget -= line.len();
        strip_line_ending(&mut line);
        lines.push(line);
    }
    let more = !reader.fill_buf()?.is_empty();
    Ok(Collected::Lines { lines, more })
}

fn strip_line_ending(line: &mut Vec<u8>) {
    if line.ends_with(b"\n") {
        line.pop();
        if line.ends_with(b"\r") {
            line.pop();
        }
    }
}

/// Consumes the rest of the line at the reader and returns its length, without its line ending.
fn skip_rest_of_line(reader: &mut impl BufRead) -> std::io::Result<usize> {
    let mut len = 0;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(len);
        }
        if let Some(at) = memchr::memchr(b'\n', available) {
            reader.consume(at + 1);
            return Ok(len + at);
        }
        let read = available.len();
        reader.consume(read);
        len += read;
    }
}

/// Shows the start of line `line_no`, `head`, cut on a character boundary.
fn oversized_line(
    mut reader: impl BufRead,
    head: Vec<u8>,
    line_no: usize,
    clean_path: &str,
) -> std::io::Result<ToolResult> {
    let line_len = head.len() + skip_rest_of_line(&mut reader)?;
    let more = !reader.fill_buf()?.is_empty();
    let shown = match std::str::from_utf8(&head) {
        Ok(text) => text,
        Err(error) if error.error_len().is_none() => std::str::from_utf8(&head[..error.valid_up_to()]).unwrap_or(""),
        Err(_) => return Ok(invalid_utf8(clean_path)),
    };
    Ok(super::format::oversized_line_result(shown, line_no, line_len, more))
}

fn invalid_utf8(clean_path: &str) -> ToolResult {
    ToolResult::error(format!("File contains invalid UTF-8: {clean_path}"))
}
