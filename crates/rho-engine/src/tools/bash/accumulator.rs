use super::normalize::BASH_CLEAN_HEAD_LINES;
use crate::tools::truncate::{DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, Truncation, truncate_tail};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const BASH_PRUNE_LINE_THRESHOLD: usize = 15;
pub const BASH_MAX_LOG_BYTES: usize = 16 * 1024 * 1024;
const HEAD_MAX_BYTES: usize = 4 * 1024;
const LOG_DIR_NAME: &str = "rho-bash";
const LOG_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const LOG_MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
/// Logs this young may still be read by a live session (the footer path is handed to the model),
/// possibly in another rho process, so the size cap never evicts them.
const LOG_SPARE_WINDOW: Duration = Duration::from_secs(60 * 60);

/// Where full output logs go unless a tool is given its own directory.
pub fn default_log_dir() -> PathBuf {
    std::env::temp_dir().join(LOG_DIR_NAME)
}

/// Removes full output logs under `temp_root` older than a day or beyond the total size cap, and
/// day-old `rho-bash-*.log` files that earlier versions left loose in `temp_root`. Returns the count.
pub fn sweep_logs(temp_root: &Path) -> usize {
    let removed = crate::tools::artifact::cleanup_artifacts(
        &temp_root.join(LOG_DIR_NAME),
        LOG_MAX_AGE,
        LOG_MAX_TOTAL_BYTES,
        LOG_SPARE_WINDOW,
    )
    .unwrap_or(0);
    removed + remove_loose_logs(temp_root)
}

fn remove_loose_logs(temp_root: &Path) -> usize {
    let cutoff = SystemTime::now()
        .checked_sub(LOG_MAX_AGE)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let Ok(entries) = std::fs::read_dir(temp_root) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("rho-bash-") && name.ends_with(".log")
        })
        .filter(|entry| {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|modified| modified < cutoff)
        })
        .filter(|entry| std::fs::remove_file(entry.path()).is_ok())
        .count()
}

#[derive(Debug, Clone)]
pub struct OutputSnapshot {
    pub content: String,
    /// The output's first whole lines when the output was clipped to its tail.
    pub head: Option<String>,
    pub truncation: Truncation,
    pub full_output_path: Option<PathBuf>,
}

pub struct OutputAccumulator {
    max_lines: usize,
    max_bytes: usize,
    max_rolling_bytes: usize,
    max_log_bytes: usize,
    log_bytes: usize,
    log_truncated: bool,
    tail_text: String,
    head_text: String,
    head_complete: bool,
    total_raw_bytes: usize,
    total_lines: usize,
    has_open_line: bool,
    finished: bool,
    temp_file_path: Option<PathBuf>,
    temp_file_writer: Option<BufWriter<File>>,
    /// Plain log text held until the output is long enough to need a log file.
    unwritten_log: String,
    pending_ansi: String,
    log_dir: PathBuf,
}

impl Default for OutputAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputAccumulator {
    pub fn new() -> Self {
        Self {
            max_lines: DEFAULT_MAX_LINES,
            max_bytes: DEFAULT_MAX_BYTES,
            max_rolling_bytes: DEFAULT_MAX_BYTES * 2,
            max_log_bytes: BASH_MAX_LOG_BYTES,
            log_bytes: 0,
            log_truncated: false,
            tail_text: String::new(),
            head_text: String::new(),
            head_complete: false,
            total_raw_bytes: 0,
            total_lines: 0,
            has_open_line: false,
            finished: false,
            temp_file_path: None,
            temp_file_writer: None,
            unwritten_log: String::new(),
            pending_ansi: String::new(),
            log_dir: default_log_dir(),
        }
    }

    #[must_use]
    pub fn with_log_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.log_dir = dir.into();
        self
    }

    #[cfg(test)]
    pub(crate) fn with_log_limit(mut self, bytes: usize) -> Self {
        self.max_log_bytes = bytes;
        self
    }

    pub fn append(&mut self, data: &[u8]) {
        if self.finished || data.is_empty() {
            return;
        }
        self.total_raw_bytes = self.total_raw_bytes.saturating_add(data.len());
        let text = if self.pending_ansi.is_empty() {
            String::from_utf8_lossy(data).into_owned()
        } else {
            let mut s = std::mem::take(&mut self.pending_ansi);
            s.push_str(&String::from_utf8_lossy(data));
            s
        };
        let (complete, pending) = super::sanitize::split_at_incomplete_ansi(&text);
        if !pending.is_empty() {
            self.pending_ansi = pending.to_string();
        }
        if !complete.is_empty() {
            let sanitized = super::sanitize::sanitize_binary_output(complete);
            self.append_decoded_text(&sanitized);
            self.log(&sanitized);
        }
    }

    pub fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        if !self.pending_ansi.is_empty() {
            let leftover = std::mem::take(&mut self.pending_ansi);
            let sanitized = super::sanitize::sanitize_binary_output(&leftover);
            self.append_decoded_text(&sanitized);
            self.log(&sanitized);
        }
        if self.should_use_temp_file() {
            self.ensure_temp_file();
        }
        if let Some(mut writer) = self.temp_file_writer.take() {
            let _ = writer.flush();
        }
    }

    pub fn snapshot(&self) -> OutputSnapshot {
        let mut truncation = truncate_tail(&self.tail_text, self.max_lines, self.max_bytes);
        truncation.total_lines = self.total_lines.max(truncation.output_lines);
        truncation.total_bytes = self.total_raw_bytes.max(truncation.output_bytes);
        let truncated = self.total_lines > self.max_lines || self.total_raw_bytes > self.max_bytes;
        truncation.truncated = truncated;

        let head = self.clipped_head(&truncation);
        OutputSnapshot {
            content: truncation.content.clone(),
            head,
            truncation,
            full_output_path: self.temp_file_path.clone(),
        }
    }

    fn clipped_head(&self, truncation: &Truncation) -> Option<String> {
        let head = self.head_text.trim_end_matches('\n');
        (truncation.truncated && !truncation.last_line_partial && !head.is_empty()).then(|| head.to_string())
    }

    fn capture_head(&mut self, text: &str) {
        if self.head_complete {
            return;
        }
        let wanted = BASH_CLEAN_HEAD_LINES - self.head_text.matches('\n').count();
        let (end, complete) = match text.match_indices('\n').nth(wanted - 1) {
            Some((index, _)) => (index + 1, true),
            None => (text.len(), false),
        };
        if self.head_text.len() + end > HEAD_MAX_BYTES {
            self.head_complete = true;
            let whole_lines = self.head_text.rfind('\n').map_or(0, |index| index + 1);
            self.head_text.truncate(whole_lines);
            return;
        }
        self.head_complete = complete;
        self.head_text.push_str(&text[..end]);
    }

    fn append_decoded_text(&mut self, text: &str) {
        self.capture_head(text);
        self.tail_text.push_str(text);
        if self.tail_text.len() > self.max_rolling_bytes * 2 {
            self.trim_tail();
        }
        let newlines = text.bytes().filter(|&b| b == b'\n').count();
        self.has_open_line = !text.ends_with('\n');
        self.total_lines = self.total_lines.saturating_add(newlines);
    }

    fn trim_tail(&mut self) {
        if self.tail_text.len() <= self.max_rolling_bytes {
            return;
        }
        let target_start = self.tail_text.len().saturating_sub(self.max_rolling_bytes);
        let mut boundary = self.tail_text.ceil_char_boundary(target_start);
        if let Some(next_newline) = self.tail_text[boundary..].find('\n') {
            boundary += next_newline + 1;
        }
        self.tail_text = self.tail_text[boundary..].to_string();
    }

    /// The log is read back by the model, so it gets the decoded text without terminal colors.
    fn log(&mut self, text: &str) {
        let plain = super::sanitize::strip_ansi(text);
        if self.temp_file_writer.is_some() || self.should_use_temp_file() {
            self.ensure_temp_file();
            self.write_log(&plain);
        } else {
            self.unwritten_log.push_str(&plain);
        }
    }

    fn should_use_temp_file(&self) -> bool {
        self.total_raw_bytes > self.max_bytes
            || self.total_lines > self.max_lines
            || self.total_lines > BASH_PRUNE_LINE_THRESHOLD
    }

    fn ensure_temp_file(&mut self) {
        if self.temp_file_writer.is_some() {
            return;
        }
        let dir = &self.log_dir;
        let path = dir.join(format!("{}.log", &uuid::Uuid::new_v4().simple().to_string()[..12]));
        let Ok(file) = super::log::create_private_dir(dir).and_then(|()| super::log::create_log_file(&path)) else {
            return;
        };
        self.temp_file_path = Some(path);
        self.temp_file_writer = Some(BufWriter::new(file));
        let unwritten = std::mem::take(&mut self.unwritten_log);
        self.write_log(&unwritten);
    }

    fn write_log(&mut self, data: &str) {
        if self.log_truncated {
            return;
        }
        let Some(writer) = &mut self.temp_file_writer else {
            return;
        };
        let remaining = self.max_log_bytes.saturating_sub(self.log_bytes);
        if data.len() <= remaining {
            let _ = writer.write_all(data.as_bytes());
            self.log_bytes += data.len();
            return;
        }
        let _ = writer.write_all(&data.as_bytes()[..data.floor_char_boundary(remaining)]);
        let _ = writer.write_all(format!("\n[log truncated at {} bytes]\n", self.max_log_bytes).as_bytes());
        self.log_bytes = self.max_log_bytes;
        self.log_truncated = true;
    }
}
