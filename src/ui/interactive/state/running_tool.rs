use std::time::Instant;

pub const MAX_RUNNING_OUTPUT_BYTES: usize = 50 * 1024;
pub const MAX_RUNNING_BUFFER_BYTES: usize = 100 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningTool {
    pub name: String,
    pub args_summary: String,
    pub started: Instant,
    pub output: String,
    pub preview: Option<String>,
}

impl RunningTool {
    pub const MAX_RUNNING_OUTPUT_BYTES: usize = MAX_RUNNING_OUTPUT_BYTES;
    pub const MAX_RUNNING_BUFFER_BYTES: usize = MAX_RUNNING_BUFFER_BYTES;

    pub fn new(name: impl Into<String>, args_summary: impl Into<String>, preview: Option<String>) -> Self {
        Self {
            name: name.into(),
            args_summary: args_summary.into(),
            started: Instant::now(),
            output: String::new(),
            preview,
        }
    }

    pub fn append_chunk(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        let chunk = if chunk.len() > Self::MAX_RUNNING_BUFFER_BYTES * 2 {
            let mut start = chunk.len().saturating_sub(Self::MAX_RUNNING_BUFFER_BYTES * 2);
            while start < chunk.len() && !chunk.is_char_boundary(start) {
                start += 1;
            }
            &chunk[start..]
        } else {
            chunk
        };
        self.output.push_str(chunk);
        if self.output.len() > Self::MAX_RUNNING_BUFFER_BYTES {
            self.trim_tail();
        }
    }

    pub fn trim_tail(&mut self) {
        if self.output.len() <= Self::MAX_RUNNING_OUTPUT_BYTES {
            return;
        }
        let target_start = self.output.len().saturating_sub(Self::MAX_RUNNING_OUTPUT_BYTES);
        let mut boundary = target_start;
        while boundary < self.output.len() && !self.output.is_char_boundary(boundary) {
            boundary += 1;
        }
        if let Some(next_newline) = self.output[boundary..].find('\n') {
            let newline_idx = boundary + next_newline + 1;
            if newline_idx < self.output.len() {
                boundary = newline_idx;
            }
        }
        self.output = self.output[boundary..].to_string();
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }
}
