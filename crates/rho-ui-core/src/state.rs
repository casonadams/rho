use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub fn format_tokens(count: u64) -> String {
    if count < 1_000 {
        count.to_string()
    } else if count < 100_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else if count < 1_000_000 {
        format!("{}k", (count as f64 / 1_000.0).round() as u64)
    } else if count.is_multiple_of(1_000_000) {
        format!("{}M", count / 1_000_000)
    } else {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    }
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else if bytes < GB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    }
}

pub fn detect_supported_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        None
    }
}

pub fn fit_dimensions(width: u32, height: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if width <= max_w && height <= max_h {
        return (width, height);
    }
    let ratio_w = max_w as f64 / width as f64;
    let ratio_h = max_h as f64 / height as f64;
    let ratio = ratio_w.min(ratio_h);
    let new_w = ((width as f64 * ratio).round() as u32).max(1);
    let new_h = ((height as f64 * ratio).round() as u32).max(1);
    (new_w, new_h)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FooterMetrics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub context_tokens: usize,
    pub context_window: usize,
    pub total_cost: Option<f64>,
    pub tokens_per_second: Option<f64>,
    pub quota_summary: Option<String>,
}

impl FooterMetrics {
    pub fn context_percent(&self) -> f64 {
        if self.context_window == 0 {
            0.0
        } else {
            (self.context_tokens as f64 / self.context_window as f64) * 100.0
        }
    }

    pub fn format_stats_line(&self, model: &str) -> String {
        let mut parts = Vec::new();
        parts.push(format!("↑{}", format_tokens(self.input_tokens)));
        parts.push(format!("↓{}", format_tokens(self.output_tokens)));
        if self.cache_read_tokens > 0 {
            parts.push(format!("R{}", format_tokens(self.cache_read_tokens)));
        }
        if self.cache_write_tokens > 0 {
            parts.push(format!("W{}", format_tokens(self.cache_write_tokens)));
        }
        if let Some(cost) = self.total_cost {
            parts.push(format!("${cost:.3}"));
        }
        if self.context_window > 0 {
            parts.push(format!(
                "{:.1}%/{}",
                self.context_percent(),
                format_tokens(self.context_window as u64)
            ));
        }
        if let Some(tps) = self.tokens_per_second
            && tps >= 1.0
        {
            parts.push(format!("@{tps:.0}t/s"));
        }
        parts.push(model.to_string());
        parts.join(" · ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RhoTicket {
    pub node_id: String,
    pub relay_url: Option<String>,
}

impl RhoTicket {
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        let stripped = trimmed.strip_prefix("ticket:").unwrap_or(trimmed);
        if stripped.is_empty() {
            return None;
        }

        if let Some((node, relay)) = stripped.split_once('@') {
            Some(Self {
                node_id: node.to_string(),
                relay_url: Some(relay.to_string()),
            })
        } else {
            Some(Self {
                node_id: stripped.to_string(),
                relay_url: None,
            })
        }
    }

    pub fn to_string_repr(&self) -> String {
        if let Some(relay) = &self.relay_url {
            format!("ticket:{}@{relay}", self.node_id)
        } else {
            format!("ticket:{}", self.node_id)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub label: String,
    pub timestamp: DateTime<Utc>,
    pub is_checkpoint: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTreeState {
    pub nodes: Vec<TreeNode>,
    pub active_node_id: Option<String>,
}

impl SessionTreeState {
    pub fn add_node(&mut self, id: String, parent_id: Option<String>, label: String, is_checkpoint: bool) {
        self.nodes.push(TreeNode {
            id: id.clone(),
            parent_id,
            label,
            timestamp: Utc::now(),
            is_checkpoint,
        });
        self.active_node_id = Some(id);
    }

    pub fn switch_branch(&mut self, node_id: &str) -> bool {
        if self.nodes.iter().any(|n| n.id == node_id) {
            self.active_node_id = Some(node_id.to_string());
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WelcomeDisplay {
    pub version: String,
    pub working_dir: String,
    pub git_branch: Option<String>,
    pub active_model: String,
    pub mcp_servers_count: usize,
    pub skills_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowFocus {
    #[default]
    Focused,
    Unfocused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToastMessage {
    pub id: u64,
    pub level: ToastLevel,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub duration_ms: u64,
}

#[derive(Debug, Default)]
pub struct ToastManager {
    next_id: u64,
    pub active_toasts: Vec<ToastMessage>,
}

impl ToastManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, level: ToastLevel, message: impl Into<String>, duration_ms: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.active_toasts.push(ToastMessage {
            id,
            level,
            message: message.into(),
            timestamp: Utc::now(),
            duration_ms,
        });
        id
    }

    pub fn dismiss(&mut self, id: u64) {
        self.active_toasts.retain(|t| t.id != id);
    }

    pub fn prune_expired(&mut self, now: DateTime<Utc>) {
        self.active_toasts.retain(|t| {
            let elapsed = (now - t.timestamp).num_milliseconds();
            elapsed >= 0 && (elapsed as u64) < t.duration_ms
        });
    }
}

pub struct SecretGuard;

impl SecretGuard {
    pub fn redact(text: &str) -> String {
        let mut words = Vec::new();
        for word in text.split(' ') {
            let clean = word.trim_matches(['"', '\'', '`', '(', ')', ';', ',']);
            if clean.starts_with("sk-ant-") {
                words.push(word.replace(clean, "sk-ant-••••••••"));
            } else if clean.starts_with("sk-proj-") {
                words.push(word.replace(clean, "sk-proj-••••••••"));
            } else if clean.starts_with("sk-") && clean.len() > 8 {
                words.push(word.replace(clean, "sk-••••••••"));
            } else if clean.starts_with("ghp_") || clean.starts_with("gho_") {
                let prefix = &clean[..4];
                words.push(word.replace(clean, &format!("{prefix}••••••••")));
            } else {
                words.push(word.to_string());
            }
        }
        words.join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueuedPromptKind {
    FollowUp,
    SteeringInterrupt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedPrompt {
    pub text: String,
    pub kind: QueuedPromptKind,
}

#[derive(Debug, Default)]
pub struct PromptQueueCoordinator {
    queue: VecDeque<QueuedPrompt>,
}

impl PromptQueueCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue_follow_up(&mut self, text: impl Into<String>) {
        self.queue.push_back(QueuedPrompt {
            text: text.into(),
            kind: QueuedPromptKind::FollowUp,
        });
    }

    pub fn enqueue_steering(&mut self, text: impl Into<String>) {
        self.queue.push_front(QueuedPrompt {
            text: text.into(),
            kind: QueuedPromptKind::SteeringInterrupt,
        });
    }

    pub fn pop_next(&mut self) -> Option<QueuedPrompt> {
        self.queue.pop_front()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}
