use crate::tools::truncate::{
    DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, TruncatedBy, Truncation, format_size, truncate_tail,
};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct OutputSnapshot {
    pub content: String,
    pub formatted_text: String,
    pub truncation: Truncation,
    pub full_output_path: Option<PathBuf>,
}

pub struct OutputAccumulator {
    max_lines: usize,
    max_bytes: usize,
    max_rolling_bytes: usize,
    temp_file_prefix: String,
    tail_text: String,
    total_raw_bytes: usize,
    total_lines: usize,
    current_line_bytes: usize,
    has_open_line: bool,
    finished: bool,
    temp_file_path: Option<PathBuf>,
    temp_file_writer: Option<BufWriter<File>>,
    raw_chunks: Vec<Vec<u8>>,
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
            temp_file_prefix: "rho-bash".to_string(),
            tail_text: String::new(),
            total_raw_bytes: 0,
            total_lines: 0,
            current_line_bytes: 0,
            has_open_line: false,
            finished: false,
            temp_file_path: None,
            temp_file_writer: None,
            raw_chunks: Vec::new(),
        }
    }

    pub fn append(&mut self, data: &[u8]) {
        if self.finished || data.is_empty() {
            return;
        }
        self.total_raw_bytes = self.total_raw_bytes.saturating_add(data.len());
        let text = String::from_utf8_lossy(data);
        let sanitized = super::sanitize::sanitize_binary_output(&text);
        self.append_decoded_text(&sanitized);

        if self.temp_file_writer.is_some() || self.should_use_temp_file() {
            self.ensure_temp_file();
            if let Some(writer) = &mut self.temp_file_writer {
                let _ = writer.write_all(data);
            }
        } else {
            self.raw_chunks.push(data.to_vec());
        }
    }

    pub fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
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

        let formatted_text = self.format_snapshot_text(&truncation);
        OutputSnapshot {
            content: truncation.content.clone(),
            formatted_text,
            truncation,
            full_output_path: self.temp_file_path.clone(),
        }
    }

    fn append_decoded_text(&mut self, text: &str) {
        self.tail_text.push_str(text);
        if self.tail_text.len() > self.max_rolling_bytes * 2 {
            self.trim_tail();
        }
        let newlines = text.bytes().filter(|&b| b == b'\n').count();
        if newlines == 0 {
            self.current_line_bytes = self.current_line_bytes.saturating_add(text.len());
            self.has_open_line = true;
        } else {
            let last_newline_pos = text.rfind('\n').unwrap_or(0);
            let tail = &text[last_newline_pos + 1..];
            self.current_line_bytes = tail.len();
            self.has_open_line = !tail.is_empty();
        }
        self.total_lines = self.total_lines.saturating_add(newlines);
    }

    fn trim_tail(&mut self) {
        if self.tail_text.len() <= self.max_rolling_bytes {
            return;
        }
        let target_start = self.tail_text.len().saturating_sub(self.max_rolling_bytes);
        let mut boundary = target_start;
        while boundary < self.tail_text.len() && !self.tail_text.is_char_boundary(boundary) {
            boundary += 1;
        }
        if let Some(next_newline) = self.tail_text[boundary..].find('\n') {
            boundary += next_newline + 1;
        }
        self.tail_text = self.tail_text[boundary..].to_string();
    }

    fn should_use_temp_file(&self) -> bool {
        self.total_raw_bytes > self.max_bytes || self.total_lines > self.max_lines
    }

    fn ensure_temp_file(&mut self) {
        if self.temp_file_writer.is_some() {
            return;
        }
        let id = uuid::Uuid::new_v4();
        let path = std::env::temp_dir().join(format!("{}-{id}.log", self.temp_file_prefix));
        if let Ok(file) = File::create(&path) {
            let mut writer = BufWriter::new(file);
            for chunk in self.raw_chunks.drain(..) {
                let _ = writer.write_all(&chunk);
            }
            self.temp_file_path = Some(path);
            self.temp_file_writer = Some(writer);
        }
    }

    fn format_snapshot_text(&self, truncation: &Truncation) -> String {
        let mut text = truncation.content.clone();
        if truncation.truncated {
            let path_str = self
                .temp_file_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "temp log".to_string());
            let end_line = truncation.total_lines;
            let start_line = truncation
                .total_lines
                .saturating_sub(truncation.output_lines)
                .saturating_add(1);

            if truncation.last_line_partial {
                let size = format_size(truncation.output_bytes);
                let line_size = format_size(self.current_line_bytes);
                text.push_str(&format!(
                    "\n\n[Showing last {size} of line {end_line} (line is {line_size}). Full output: {path_str}]"
                ));
            } else if truncation.truncated_by == Some(TruncatedBy::Lines) {
                text.push_str(&format!(
                    "\n\n[Showing lines {start_line}-{end_line} of {}. Full output: {path_str}]",
                    truncation.total_lines
                ));
            } else {
                let limit = format_size(self.max_bytes);
                text.push_str(&format!(
                    "\n\n[Showing lines {start_line}-{end_line} of {} ({limit} limit). Full output: {path_str}]",
                    truncation.total_lines
                ));
            }
        }
        text
    }
}
