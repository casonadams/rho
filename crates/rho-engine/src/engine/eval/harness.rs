//! Core harness: `EvalHarness::run` plus the verify/normalize helpers it composes.

use super::mock::mock_engine;
use super::types::{EvalFailure, EvalReport, EvalScenario, NormalizedPart, NormalizedRequest, NormalizedTranscript};
use crate::engine::metrics::TerminalStatus;
use crate::engine::runner::{TurnOutput, TurnRequest};
use crate::provider::adapter::{MockAdapter, ModelCompletionRequest};
use rho_harness_core::model::ChatMessage;
use std::collections::HashMap;
use std::sync::Arc;

pub struct EvalHarness;

fn build_harness_engine(
    scenario: &EvalScenario,
    base_dir: &std::path::Path,
    model: Arc<MockAdapter>,
) -> crate::engine::AgentEngine {
    mock_engine(
        model,
        super::mock::MockEngineConfig {
            base_dir,
            app_config: rho_harness_core::config::Config {
                max_turns: scenario.max_turns,
                permission: rho_harness_core::config::PermissionConfig { enabled: false },
                ..rho_harness_core::config::Config::default()
            },
            session_manager: None,
            built_in_tools: scenario.built_in_tools.clone(),
        },
    )
}

impl EvalHarness {
    pub async fn run(scenario: EvalScenario, base_dir: &std::path::Path) -> Result<EvalReport, EvalFailure> {
        let model = Arc::new(MockAdapter::from_turns(scenario.turns.clone()));
        let engine = build_harness_engine(&scenario, base_dir, model.clone());
        let output = engine
            .run_turn(TurnRequest::new(scenario.prompt), super::presenter::presenter())
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

pub fn verify_output(scenario: &EvalScenario, output: &TurnOutput) -> Result<(), EvalFailure> {
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

pub fn verify_tool_order(scenario: &EvalScenario, transcript: &NormalizedTranscript) -> Result<(), EvalFailure> {
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

pub fn normalize_requests(requests: &[ModelCompletionRequest]) -> NormalizedTranscript {
    let mut ids = HashMap::new();
    let requests = requests
        .iter()
        .enumerate()
        .map(|(index, request)| NormalizedRequest {
            index,
            content_telemetry: false,
            messages: request
                .messages
                .iter()
                .map(|message| normalize_message(message, &mut ids))
                .collect(),
        })
        .collect();
    NormalizedTranscript { requests }
}

pub fn normalize_message(message: &ChatMessage, ids: &mut HashMap<String, String>) -> super::types::NormalizedMessage {
    match message {
        ChatMessage::System { .. } => super::types::NormalizedMessage {
            role: "system",
            parts: vec![NormalizedPart::Text],
        },
        ChatMessage::User { content } => {
            let mut parts = Vec::new();
            for part in content {
                match part {
                    rho_harness_core::model::UserContent::Text(_) => parts.push(NormalizedPart::Text),
                    rho_harness_core::model::UserContent::Image(_) => parts.push(NormalizedPart::Image),
                    rho_harness_core::model::UserContent::ToolResult(r) => parts.push(NormalizedPart::ToolResult {
                        id: normalize_id(&r.call, ids),
                        name: r.name.clone(),
                    }),
                }
            }
            super::types::NormalizedMessage { role: "user", parts }
        }
        ChatMessage::Assistant { content, .. } => {
            let mut parts = Vec::new();
            for part in content {
                match part {
                    rho_harness_core::model::AssistantContent::Text(_) => parts.push(NormalizedPart::Text),
                    rho_harness_core::model::AssistantContent::Reasoning(_) => parts.push(NormalizedPart::Reasoning),
                    rho_harness_core::model::AssistantContent::Image(_) => parts.push(NormalizedPart::Image),
                    rho_harness_core::model::AssistantContent::ToolCall(call) => parts.push(NormalizedPart::ToolCall {
                        id: normalize_id(&call.id, ids),
                        name: call.function.name.clone(),
                    }),
                }
            }
            super::types::NormalizedMessage {
                role: "assistant",
                parts,
            }
        }
    }
}

pub fn normalize_id(id: &str, ids: &mut HashMap<String, String>) -> String {
    let next = format!("call-{}", ids.len() + 1);
    ids.entry(id.to_string()).or_insert(next).clone()
}
