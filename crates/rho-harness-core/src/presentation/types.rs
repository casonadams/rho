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

/// One selectable row in a generic modal rendered on behalf of a caller
/// (plugin or engine).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionOption {
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Inline input shown at the bottom of the same modal when this option is
    /// chosen; the submitted text travels back with the selection.
    #[serde(default)]
    pub input: Option<InteractionInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionInput {
    /// Label for the input line (e.g. "edit", "pattern", "reason").
    pub label: String,
    /// Prefill for the input buffer.
    #[serde(default)]
    pub value: Option<String>,
}

/// A generic modal request; deserializable straight from plugin `ui/prompt`
/// params, so senders only need these fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionPrompt {
    pub title: String,
    pub body: String,
    pub options: Vec<InteractionOption>,
    #[serde(default)]
    pub initial_selection: usize,
    #[serde(default)]
    pub allow_custom: bool,
    /// Prefill for the custom text input (used by host/ui/input), so users
    /// edit an existing value instead of retyping it.
    #[serde(default)]
    pub initial_text: Option<String>,
}

/// Serializes as `{"selected":n}` / `{"custom":"..."}` / `"cancelled"` — the
/// reply contract that plugin `ui/prompt` requests rely on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionResponse {
    Selected(usize),
    /// Text submitted through an option's inline input; carries both so the
    /// plugin knows which action the text belongs to.
    SelectedWithInput {
        index: usize,
        text: String,
    },
    Custom(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WelcomeDisplay {
    pub model: String,
    pub provider: String,
    pub auto_approve: bool,
    pub resumed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<String>,
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
pub struct BlockDisplay {
    #[serde(default)]
    pub title: String,
    pub content: String,
    #[serde(default = "default_style")]
    pub style: String,
}

fn default_style() -> String {
    "info".to_string()
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
