//! Core harness: `EvalHarness::run` plus the verify/normalize helpers it composes.
//!
//! The split mirrors the runtime layers the harness covers:
//! - [`EvalHarness::run`] drives a scenario end-to-end (run → verify output → normalize
//!   transcript → verify tool order).
//! - [`verify_output`] / [`verify_tool_order`] assert scenario invariants on
//!   the final outcome and the recorded completion requests.
//! - [`normalize_requests`] / [`normalize_message`] / [`normalize_user_part`] /
//!   [`normalize_assistant_part`] / [`normalize_id`] produce a stable transcript
//!   representation that drops provider-specific IDs and content.

use super::mock::mock_engine;
use super::types::{EvalFailure, EvalReport, EvalScenario, NormalizedPart, NormalizedRequest, NormalizedTranscript};
use crate::engine::metrics::TerminalStatus;
use crate::engine::runner::{TurnOutput, TurnRequest};
use crate::ui::TerminalRenderer;
use rig::completion::CompletionRequest;
use rig::message::{AssistantContent, Message, UserContent};
use std::collections::HashMap;

pub(super) struct EvalHarness;

impl EvalHarness {
    pub(super) async fn run(scenario: EvalScenario, base_dir: &std::path::Path) -> Result<EvalReport, EvalFailure> {
        let model = rig::test_utils::MockCompletionModel::from_stream_turns(scenario.turns.clone());
        let engine = mock_engine(model.clone(), base_dir, scenario.max_turns);
        let output = engine
            .run_turn(TurnRequest::new(scenario.prompt), &TerminalRenderer::default())
            .await
            .map_err(|_| EvalFailure {
                scenario: scenario.name,
                behavior: "scenario execution failed",
            })?;
        verify_output(&scenario, &output)?;
        let transcript = normalize_requests(&model.requests());
        verify_tool_order(&scenario, &transcript)?;
        Ok(EvalReport {
            scenario: scenario.name,
            transcript,
            metrics: output.metrics.normalized(),
        })
    }
}

pub(super) fn verify_output(scenario: &EvalScenario, output: &TurnOutput) -> Result<(), EvalFailure> {
    if output.final_text != scenario.expected_final {
        return Err(EvalFailure {
            scenario: scenario.name,
            behavior: "final answer mismatch",
        });
    }
    if output.metrics.terminal_status != TerminalStatus::Completed {
        return Err(EvalFailure {
            scenario: scenario.name,
            behavior: "terminal status mismatch",
        });
    }
    Ok(())
}

pub(super) fn verify_tool_order(scenario: &EvalScenario, transcript: &NormalizedTranscript) -> Result<(), EvalFailure> {
    let names = transcript
        .requests
        .last()
        .map(|request| {
            request
                .messages
                .iter()
                .flat_map(|message| &message.parts)
                .filter_map(|part| match part {
                    NormalizedPart::ToolCall { name, .. } => Some(name.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if names != scenario.expected_tools {
        return Err(EvalFailure {
            scenario: scenario.name,
            behavior: "tool ordering mismatch",
        });
    }
    Ok(())
}

pub(super) fn normalize_requests(requests: &[CompletionRequest]) -> NormalizedTranscript {
    let mut ids = HashMap::new();
    let requests = requests
        .iter()
        .enumerate()
        .map(|(index, request)| NormalizedRequest {
            index,
            content_telemetry: request.record_telemetry_content,
            messages: request
                .chat_history
                .iter()
                .map(|message| normalize_message(message, &mut ids))
                .collect(),
        })
        .collect();
    NormalizedTranscript { requests }
}

pub(super) fn normalize_message(
    message: &Message,
    ids: &mut HashMap<String, String>,
) -> super::types::NormalizedMessage {
    match message {
        Message::System { .. } => super::types::NormalizedMessage {
            role: "system",
            parts: vec![NormalizedPart::Text],
        },
        Message::User { content } => super::types::NormalizedMessage {
            role: "user",
            parts: content.iter().map(|part| normalize_user_part(part, ids)).collect(),
        },
        Message::Assistant { content, .. } => super::types::NormalizedMessage {
            role: "assistant",
            parts: content.iter().map(|part| normalize_assistant_part(part, ids)).collect(),
        },
    }
}

pub(super) fn normalize_user_part(part: &UserContent, ids: &mut HashMap<String, String>) -> NormalizedPart {
    match part {
        UserContent::Text(_) => NormalizedPart::Text,
        UserContent::ToolResult(result) => NormalizedPart::ToolResult {
            id: normalize_id(result.call.as_str(), ids),
            name: result.name.clone(),
        },
        UserContent::Image(_) => NormalizedPart::Image,
        UserContent::Audio(_) => NormalizedPart::Audio,
        UserContent::Video(_) => NormalizedPart::Video,
        UserContent::Document(_) => NormalizedPart::Document,
    }
}

pub(super) fn normalize_assistant_part(part: &AssistantContent, ids: &mut HashMap<String, String>) -> NormalizedPart {
    match part {
        AssistantContent::Text(_) => NormalizedPart::Text,
        AssistantContent::ToolCall(call) => NormalizedPart::ToolCall {
            id: normalize_id(call.id.as_str(), ids),
            name: call.function.name.clone(),
        },
        AssistantContent::Reasoning(_) => NormalizedPart::Reasoning,
        AssistantContent::Image(_) => NormalizedPart::Image,
    }
}

pub(super) fn normalize_id(id: &str, ids: &mut HashMap<String, String>) -> String {
    let next = format!("call-{}", ids.len() + 1);
    ids.entry(id.to_string()).or_insert(next).clone()
}
