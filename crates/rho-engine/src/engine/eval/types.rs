//! Data shapes produced and consumed by the eval harness.
//!
//! These types are deliberately `Serialize` (where it makes sense) so reports can
//! be diffed byte-for-byte in tests, and `Debug + PartialEq` so failure messages can
//! be asserted against without exposing expected or observed content (see
//! `evaluation_errors_do_not_include_expected_or_observed_content` in `tests.rs`).

use crate::engine::metrics::RunMetrics;
use crate::provider::adapter::ModelStreamEvent;
use crate::tools::EngineTool;
use serde::Serialize;
use std::sync::Arc;

pub struct EvalScenario {
    pub name: &'static str,
    pub prompt: &'static str,
    pub turns: Vec<Vec<ModelStreamEvent>>,
    pub expected_final: &'static str,
    pub expected_tools: Vec<&'static str>,
    pub max_turns: usize,
    pub built_in_tools: Option<Vec<Arc<dyn EngineTool>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvalReport {
    pub scenario: &'static str,
    pub transcript: NormalizedTranscript,
    pub metrics: RunMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalFailure {
    pub scenario: &'static str,
    pub behavior: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalizedTranscript {
    pub requests: Vec<NormalizedRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalizedRequest {
    pub index: usize,
    pub content_telemetry: bool,
    pub messages: Vec<NormalizedMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalizedMessage {
    pub role: &'static str,
    pub parts: Vec<NormalizedPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NormalizedPart {
    Text,
    Reasoning,
    Image,
    Audio,
    Video,
    Document,
    ToolCall { id: String, name: String },
    ToolResult { id: String, name: String },
}
