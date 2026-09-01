use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const UI_EVENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskTier {
    ReadOnly,
    Mutating,
    HighRisk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalResult {
    Approved,
    ApprovedForSession,
    Denied { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WelcomeDisplay {
    pub model: String,
    pub provider: String,
    pub auto_approve: bool,
    pub resumed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStatus {
    pub model: String,
    pub provider: String,
    pub context: String,
    pub quota: Option<String>,
    pub auto_approve: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BashApproval {
    pub command: String,
    pub tier: RiskTier,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolLine {
    pub name: String,
    pub arguments: serde_json::Value,
    pub is_error: bool,
    pub output: String,
    pub output_summary: String,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolOutcome {
    pub name: String,
    pub is_error: bool,
    pub output_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiEvent {
    Welcome { display: WelcomeDisplay },
    SessionStatus { display: SessionStatus },
    Notice { text: String },
    UserBlock { input: String },
    Token { token: String },
    ThinkingToken { token: String },
    ToolStarted { name: String, arguments: Value },
    ToolChunk { name: String, chunk: String },
    ToolFinished { line: ToolLine },
    ActivityStarted { message: String },
    ActivityFinished,
    TurnStarted { prompt: String },
    TurnCompleted { status: String },
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiEnvelope {
    pub event_version: u32,
    #[serde(flatten)]
    pub event: UiEvent,
}

impl UiEnvelope {
    pub fn new(event: UiEvent) -> Self {
        Self {
            event_version: UI_EVENT_VERSION,
            event,
        }
    }
}
