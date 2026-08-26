//! Data shapes produced and consumed by the eval harness.
//!
//! These types are deliberately `Serialize` (where it makes sense) so reports can
//! be diffed byte-for-byte in tests, and `Debug + PartialEq` so failure messages can
//! be asserted against without exposing expected or observed content (see
//! `evaluation_errors_do_not_include_expected_or_observed_content` in `tests.rs`).

use crate::engine::metrics::RunMetrics;
use serde::Serialize;

pub(super) struct EvalScenario {
    pub(super) name: &'static str,
    pub(super) prompt: &'static str,
    pub(super) turns: Vec<Vec<rig::test_utils::MockStreamEvent>>,
    pub(super) expected_final: &'static str,
    pub(super) expected_tools: Vec<&'static str>,
    pub(super) max_turns: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct EvalReport {
    pub(super) scenario: &'static str,
    pub(super) transcript: NormalizedTranscript,
    pub(super) metrics: RunMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EvalFailure {
    pub(super) scenario: &'static str,
    pub(super) behavior: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct NormalizedTranscript {
    pub(super) requests: Vec<NormalizedRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct NormalizedRequest {
    pub(super) index: usize,
    pub(super) content_telemetry: bool,
    pub(super) messages: Vec<NormalizedMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct NormalizedMessage {
    pub(super) role: &'static str,
    pub(super) parts: Vec<NormalizedPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum NormalizedPart {
    Text,
    Reasoning,
    Image,
    Audio,
    Video,
    Document,
    ToolCall { id: String, name: String },
    ToolResult { id: String, name: String },
}
