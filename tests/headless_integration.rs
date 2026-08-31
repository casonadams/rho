#![cfg(unix)]

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use rho::config::Config;
use rho::engine::builder::AgentEngineBuilder;
use rho::engine::runner::TurnRequest;
use rho::platform::ToolAssembly;
use rho::presentation::{RecordingSink, StructuredPresenter, UiEnvelope, UiEvent};
use rho_sdk::capability::CapabilityError;
use rho_sdk::contract::{
    AuthenticationRequest, AuthenticationResponse, FinishReason, ProviderCapability, ProviderDescriptor,
    ProviderRequest, ProviderStreamEvent,
};
use serde_json::json;
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct TestProvider {
    turns: Mutex<VecDeque<Vec<Result<ProviderStreamEvent, CapabilityError>>>>,
}

impl TestProvider {
    fn new(turns: impl IntoIterator<Item = Vec<ProviderStreamEvent>>) -> Self {
        Self {
            turns: Mutex::new(
                turns
                    .into_iter()
                    .map(|turn| turn.into_iter().map(Ok).collect())
                    .collect(),
            ),
        }
    }
}

#[async_trait]
impl ProviderCapability for TestProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "provider:test".parse().unwrap(),
            display_name: "Test Provider".to_string(),
            models: Vec::new(),
            authentication: Vec::new(),
        }
    }

    async fn authenticate(&self, _request: AuthenticationRequest) -> Result<AuthenticationResponse, CapabilityError> {
        unreachable!()
    }

    async fn stream(
        &self,
        _request: ProviderRequest,
    ) -> Result<BoxStream<'static, Result<ProviderStreamEvent, CapabilityError>>, CapabilityError> {
        let turn = self.turns.lock().unwrap().pop_front().unwrap_or_default();
        Ok(Box::pin(stream::iter(turn)))
    }
}

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("headless_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn headless_presentation_records_deterministic_event_sequence() {
    let workspace = temp_workspace();
    let file_path = workspace.join("sample.txt");
    std::fs::write(&file_path, "headless content").unwrap();

    let provider = Arc::new(TestProvider::new([
        vec![
            ProviderStreamEvent::ToolCall {
                call_id: "call_1".to_string(),
                tool_id: "tool:read".parse().unwrap(),
                arguments: json!({"path": file_path.to_str().unwrap()}),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::ToolCalls,
            },
        ],
        vec![
            ProviderStreamEvent::TextDelta {
                text: "The file contains: headless content".to_string(),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::Stop,
            },
        ],
    ]));

    let config = Config {
        auto_approve: true,
        ..Config::default()
    };

    let active_tools = Arc::new(
        rho::plugin::tool_dispatch::ActiveToolSet::load(&config, &workspace)
            .await
            .unwrap(),
    );
    let neutral_executor = Arc::new(active_tools.neutral_executor(rig::tool::ToolContext::default()));
    let tool_assembly = ToolAssembly {
        rig_tools: Vec::new(),
        neutral_executor,
        contexts: Vec::new(),
        commands: BTreeMap::new(),
        lifecycles: Vec::new(),
    };

    let engine = AgentEngineBuilder::new(config, rho::auth::AuthStore::default())
        .base_dir(workspace.clone())
        .tool_assembly(tool_assembly.rig_tools, tool_assembly.neutral_executor)
        .provider(provider)
        .build()
        .await
        .unwrap();

    let recording = RecordingSink::new();
    let presenter = Arc::new(StructuredPresenter::recording(recording.clone()));

    let output = engine
        .run_turn(TurnRequest::new("read sample.txt"), presenter)
        .await
        .unwrap();

    assert_eq!(output.final_text, "The file contains: headless content");

    let events = recording.events();
    assert!(!events.is_empty());

    let has_tool_started = events.iter().any(|e| match e {
        UiEvent::ToolStarted { name, .. } => name == "read",
        _ => false,
    });
    assert!(has_tool_started, "Expected ToolStarted event for read");

    let has_tool_finished = events.iter().any(|e| match e {
        UiEvent::ToolFinished { line } => line.name == "read" && !line.is_error,
        _ => false,
    });
    assert!(has_tool_finished, "Expected ToolFinished event for read");

    let has_token = events.iter().any(|e| match e {
        UiEvent::Token { token } => token.contains("headless content"),
        _ => false,
    });
    assert!(has_token, "Expected Token event with response");

    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn headless_approval_denies_unapproved_operations_without_hanging() {
    let workspace = temp_workspace();
    let provider = Arc::new(TestProvider::new([
        vec![
            ProviderStreamEvent::ToolCall {
                call_id: "call_bash".to_string(),
                tool_id: "tool:bash".parse().unwrap(),
                arguments: json!({"command": "rm -rf /tmp/test"}),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::ToolCalls,
            },
        ],
        vec![
            ProviderStreamEvent::TextDelta {
                text: "command was denied".to_string(),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::Stop,
            },
        ],
    ]));

    let config = Config {
        auto_approve: false,
        ..Config::default()
    };

    let active_tools = Arc::new(
        rho::plugin::tool_dispatch::ActiveToolSet::load(&config, &workspace)
            .await
            .unwrap(),
    );
    let neutral_executor = Arc::new(active_tools.neutral_executor(rig::tool::ToolContext::default()));
    let tool_assembly = ToolAssembly {
        rig_tools: Vec::new(),
        neutral_executor,
        contexts: Vec::new(),
        commands: BTreeMap::new(),
        lifecycles: Vec::new(),
    };

    let engine = AgentEngineBuilder::new(config, rho::auth::AuthStore::default())
        .base_dir(workspace.clone())
        .tool_assembly(tool_assembly.rig_tools, tool_assembly.neutral_executor)
        .provider(provider)
        .build()
        .await
        .unwrap();

    let recording = RecordingSink::new();
    let presenter = Arc::new(StructuredPresenter::recording(recording.clone()));

    let output = engine
        .run_turn(TurnRequest::new("delete files"), presenter)
        .await
        .unwrap();

    assert_eq!(output.final_text, "command was denied");

    let events = recording.events();
    let has_approval_prompt = events.iter().any(|e| matches!(e, UiEvent::BashApprovalPrompt { .. }));
    assert!(has_approval_prompt, "Expected BashApprovalPrompt event");

    let has_tool_finished_error = events.iter().any(|e| match e {
        UiEvent::ToolFinished { line } => line.name == "bash" && line.is_error,
        _ => false,
    });
    assert!(
        has_tool_finished_error,
        "Expected ToolFinished with is_error=true due to denial"
    );

    let _ = std::fs::remove_dir_all(workspace);
}

#[test]
fn ndjson_envelope_serialization_roundtrips() {
    let event = UiEvent::Notice {
        text: "headless notice".to_string(),
    };
    let envelope = UiEnvelope::new(event);
    let serialized = serde_json::to_string(&envelope).unwrap();
    assert!(serialized.contains(r#""event_version":1"#));
    assert!(serialized.contains(r#""kind":"notice""#));
    assert!(serialized.contains(r#""text":"headless notice""#));

    let deserialized: UiEnvelope = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized.event_version, 1);
    assert_eq!(
        deserialized.event,
        UiEvent::Notice {
            text: "headless notice".to_string()
        }
    );
}
