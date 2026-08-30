//! The presentation (`ui`) capability: a versioned, ownership-neutral render
//! vocabulary. Bundled presentation plugins implement `rho-core`'s `Presenter`
//! directly; external hosts or out-of-process UIs serialize the same shapes
//! through `UiEnvelope`, so the event sequence a turn produces is identical
//! regardless of the active presenter.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Schema version of the presentation event vocabulary. Bump on any
/// incompatible change to `UiEvent`; consumers reject unknown versions
/// instead of guessing.
pub const UI_EVENT_VERSION: u32 = 1;

/// Risk classification carried on approval prompts, shared with the host's
/// safety-floor classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskTier {
    ReadOnly,
    Mutating,
    HighRisk,
}

/// Outcome of an approval prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalResult {
    Approved,
    ApprovedForSession,
    Denied { reason: String },
}

/// Display inputs for the session welcome banner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WelcomeDisplay {
    pub model: String,
    pub provider: String,
    pub auto_approve: bool,
    pub resumed: bool,
}

/// Display inputs for the live session status line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStatus {
    pub model: String,
    pub provider: String,
    pub context: String,
    pub quota: Option<String>,
    pub auto_approve: bool,
}

/// Inputs for the bash-command approval prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BashApproval {
    pub command: String,
    pub tier: RiskTier,
    pub reasons: Vec<String>,
}

/// Inputs for rendering a finished tool execution line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolLine {
    pub name: String,
    pub arguments: serde_json::Value,
    pub is_error: bool,
    pub output: String,
    pub output_summary: String,
    pub duration_ms: Option<u64>,
}

/// Inputs for rendering a tool-completion summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolOutcome {
    pub name: String,
    pub is_error: bool,
    pub output_summary: String,
}

/// The presentation event sequence: exactly what any presenter observes for a
/// turn, in order.
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
    ApprovalPrompt { tool_name: String, arguments: Value },
    BashApprovalPrompt { request: BashApproval },
    ContinueBudgetPrompt { max_turns: usize },
    Error { message: String },
}

/// Wire envelope; carries the vocabulary version next to the event so
/// consumers can reject unknown schema revisions explicitly.
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
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_sequence_roundtrips_with_event_version_stamped() {
        let events = vec![
            UiEvent::Welcome {
                display: WelcomeDisplay {
                    model: "claude-sonnet-4-6".to_string(),
                    provider: "anthropic".to_string(),
                    auto_approve: false,
                    resumed: true,
                },
            },
            UiEvent::TurnStarted {
                prompt: "summarize repo".to_string(),
            },
            UiEvent::ThinkingToken {
                token: "think".to_string(),
            },
            UiEvent::ToolStarted {
                name: "bash".to_string(),
                arguments: json!({"command": "ls"}),
            },
            UiEvent::ToolChunk {
                name: "bash".to_string(),
                chunk: "out".to_string(),
            },
            UiEvent::ToolFinished {
                line: ToolLine {
                    name: "bash".to_string(),
                    arguments: json!({"command": "ls"}),
                    is_error: false,
                    output: ".specs\nsrc".to_string(),
                    output_summary: ".specs\nsrc".to_string(),
                    duration_ms: Some(12),
                },
            },
            UiEvent::TurnCompleted {
                status: "completed".to_string(),
            },
        ];
        for event in events {
            let envelope = UiEnvelope::new(event.clone());
            let encoded = serde_json::to_string(&envelope).unwrap();
            let decoded: UiEnvelope = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded.event_version, UI_EVENT_VERSION);
            assert_eq!(decoded.event, event);
        }
    }

    #[test]
    fn unknown_event_versions_are_explicit_on_decode() {
        let raw = r#"{"event_version":99,"kind":"notice","text":"x"}"#;
        let decoded: UiEnvelope = serde_json::from_str(raw).unwrap();
        assert_eq!(decoded.event_version, 99);
        assert_eq!(decoded.event, UiEvent::Notice { text: "x".to_string() });
    }

    #[test]
    fn approval_results_roundtrip_as_snake_case() {
        let denied = UiEvent::Error {
            message: "denied: outside workspace".to_string(),
        };
        let encoded = serde_json::to_string(&denied).unwrap();
        assert!(encoded.contains(r#""kind":"error""#));
        let result = serde_json::to_string(&ApprovalResult::ApprovedForSession).unwrap();
        assert_eq!(result, r#""approved_for_session""#);
    }
}
