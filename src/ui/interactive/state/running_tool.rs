use std::time::Instant;

pub const MAX_RUNNING_OUTPUT_BYTES: usize = 50 * 1024;
pub const MAX_RUNNING_BUFFER_BYTES: usize = 100 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningTool {
    pub name: String,
    pub args_summary: String,
    pub arguments: serde_json::Value,
    pub started: Instant,
    pub output: String,
    /// The state of the file a `write` or `edit` changes, taken before it runs, so the card reads the
    /// same while it runs as when it finishes.
    pub file_change: Option<rho_harness_core::presentation::FileChange>,
    pending_ansi: String,
}

fn drain_or_join_ansi(pending_ansi: &mut String, chunk: &str) -> String {
    if pending_ansi.is_empty() {
        chunk.to_string()
    } else {
        let mut s = std::mem::take(pending_ansi);
        s.push_str(chunk);
        s
    }
}

fn clamp_sanitized_chunk(sanitized_str: &str, max_bytes: usize) -> &str {
    &sanitized_str[sanitized_str.ceil_char_boundary(sanitized_str.len().saturating_sub(max_bytes))..]
}

impl RunningTool {
    pub const MAX_RUNNING_OUTPUT_BYTES: usize = MAX_RUNNING_OUTPUT_BYTES;
    pub const MAX_RUNNING_BUFFER_BYTES: usize = MAX_RUNNING_BUFFER_BYTES;

    pub fn has_live_card(name: &str) -> bool {
        matches!(name, "bash" | "edit" | "subagent" | "workflow" | "write")
    }

    pub fn has_transcript_card(name: &str) -> bool {
        name != "wait"
    }

    /// A card for a tool that has just started. A `write` shows the file it is writing as its
    /// output, so its live card tails the content like command output.
    pub fn new(name: impl Into<String>, args_summary: impl Into<String>, arguments: serde_json::Value) -> Self {
        let mut tool = Self {
            name: name.into(),
            args_summary: args_summary.into(),
            arguments,
            started: Instant::now(),
            output: String::new(),
            file_change: None,
            pending_ansi: String::new(),
        };
        if tool.name == "write"
            && let Some(content) = rho_harness_core::args::write_text(&tool.arguments).map(str::to_string)
        {
            tool.append_chunk(&content);
        }
        tool
    }

    /// The card a start request opens.
    pub fn start(request: crate::ui::interactive::ToolStartRequest) -> Self {
        let mut tool = Self::new(request.name, request.args_summary, request.arguments);
        tool.file_change = request.file_change;
        tool
    }

    pub fn append_chunk(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        let text = drain_or_join_ansi(&mut self.pending_ansi, chunk);
        let (complete, pending) = rho_engine::tools::bash::split_at_incomplete_ansi(&text);
        if !pending.is_empty() {
            self.pending_ansi = pending.to_string();
        }
        if !complete.is_empty() {
            let sanitized = rho_engine::tools::bash::sanitize_binary_output(complete);
            let to_append = clamp_sanitized_chunk(&sanitized, Self::MAX_RUNNING_BUFFER_BYTES * 2);
            self.output.push_str(to_append);
            if self.output.len() > Self::MAX_RUNNING_BUFFER_BYTES {
                self.trim_tail();
            }
        }
    }

    pub fn trim_tail(&mut self) {
        if self.output.len() <= Self::MAX_RUNNING_OUTPUT_BYTES {
            return;
        }
        let target_start = self.output.len().saturating_sub(Self::MAX_RUNNING_OUTPUT_BYTES);
        let mut boundary = self.output.ceil_char_boundary(target_start);
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
