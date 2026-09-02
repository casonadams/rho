use chrono::{DateTime, Utc};
use rig::message::Message;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::tree::{SessionTree, TreeNodeData};
use super::super::validation::CanonicalHistory;

pub const SESSION_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionHeader {
    pub record_type: HeaderRecordType,
    pub version: u32,
    pub session_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HeaderRecordType {
    Header,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "record_type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionRecord {
    CanonicalMessages {
        sequence: u64,
        session_id: String,
        timestamp: DateTime<Utc>,
        messages: Vec<Message>,
    },
    CanonicalReset {
        sequence: u64,
        session_id: String,
        timestamp: DateTime<Utc>,
    },
    RunCheckpoint {
        sequence: u64,
        session_id: String,
        timestamp: DateTime<Utc>,
        messages: Vec<Message>,
    },
    CheckpointPromoted {
        sequence: u64,
        session_id: String,
        timestamp: DateTime<Utc>,
        messages: Vec<Message>,
    },
    AuditEvent {
        sequence: u64,
        session_id: String,
        event: SessionEvent,
    },
    TreeNode {
        sequence: u64,
        session_id: String,
        node: TreeNodeData,
    },
    ActiveLeafChanged {
        sequence: u64,
        session_id: String,
        timestamp: DateTime<Utc>,
        active_leaf_id: Option<String>,
    },
    SessionLabel {
        sequence: u64,
        session_id: String,
        timestamp: DateTime<Utc>,
        node_id: String,
        label: Option<String>,
    },
    SessionNamed {
        sequence: u64,
        session_id: String,
        timestamp: DateTime<Utc>,
        name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionEvent {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub kind: SessionEventKind,
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionEventKind {
    SystemPrompt,
    UserMessage,
    Reasoning,
    AssistantResponse,
    ToolCall,
    ToolResult,
    UsageMetrics,
    RunSummary,
    Cancellation,
    RunFailed,
}

#[derive(Debug)]
pub struct StoreState {
    pub next_sequence: u64,
    pub messages: Vec<Message>,
    pub checkpoint: Option<Vec<Message>>,
    pub events: Vec<SessionEvent>,
    pub tree: SessionTree,
    pub(crate) integrity: CanonicalHistory,
}
