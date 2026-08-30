#![cfg(unix)]

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use rho::approval::{ApprovalCapability, ApprovalDecision, ApprovalEventSink, ApprovalRequest, approval_context};
use rho::config::Config;
use rho::engine::provider::host_loop::{
    CancellationSignal, NeutralTurnRequest, NeutralTurnRuntime, NeutralTurnTerminal, NoopSteeringQueue,
    NoopTurnObserver, run_neutral_turn,
};
use rho::plugin::capability::{CapabilityError, CapabilityId};
use rho::plugin::contract::{
    AuthenticationRequest, AuthenticationResponse, FinishReason, MessageContent, MessageRole, ModelMessage,
    ProviderCapability, ProviderDescriptor, ProviderRequest, ProviderStreamEvent, ProviderToolDefinition,
};
use rho::plugin::tool_dispatch::ActiveToolSet;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct TestApprovalSink;

#[async_trait]
impl ApprovalEventSink for TestApprovalSink {
    async fn request_approval(&self, _request: ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Approved
    }
}

struct FixtureProvider {
    turns: Mutex<VecDeque<Vec<Result<ProviderStreamEvent, CapabilityError>>>>,
    requests: Mutex<Vec<ProviderRequest>>,
}

impl FixtureProvider {
    fn new(turns: impl IntoIterator<Item = Vec<ProviderStreamEvent>>) -> Self {
        Self {
            turns: Mutex::new(
                turns
                    .into_iter()
                    .map(|turn| turn.into_iter().map(Ok).collect())
                    .collect(),
            ),
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ProviderCapability for FixtureProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "provider:fixture".parse().unwrap(),
            display_name: "Fixture".to_string(),
            models: Vec::new(),
            authentication: Vec::new(),
        }
    }

    async fn authenticate(&self, _request: AuthenticationRequest) -> Result<AuthenticationResponse, CapabilityError> {
        unreachable!()
    }

    async fn stream(
        &self,
        request: ProviderRequest,
    ) -> Result<BoxStream<'static, Result<ProviderStreamEvent, CapabilityError>>, CapabilityError> {
        self.requests.lock().unwrap().push(request);
        let turn = self.turns.lock().unwrap().pop_front().unwrap_or_default();
        Ok(Box::pin(stream::iter(turn)))
    }
}

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("parallel_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn multiple_read_tools_execute_concurrently_and_preserve_order() {
    let workspace = temp_workspace();
    for i in 1..=4 {
        std::fs::write(workspace.join(format!("file_{i}.txt")), format!("content of file {i}")).unwrap();
    }

    let read_tool_id: CapabilityId = "tool:read".parse().unwrap();
    let provider = FixtureProvider::new([
        vec![
            ProviderStreamEvent::ToolCall {
                call_id: "call-read-1".to_string(),
                tool_id: read_tool_id.clone(),
                arguments: serde_json::json!({"path": workspace.join("file_1.txt").to_str().unwrap()}),
            },
            ProviderStreamEvent::ToolCall {
                call_id: "call-read-2".to_string(),
                tool_id: read_tool_id.clone(),
                arguments: serde_json::json!({"path": workspace.join("file_2.txt").to_str().unwrap()}),
            },
            ProviderStreamEvent::ToolCall {
                call_id: "call-read-3".to_string(),
                tool_id: read_tool_id.clone(),
                arguments: serde_json::json!({"path": workspace.join("file_3.txt").to_str().unwrap()}),
            },
            ProviderStreamEvent::ToolCall {
                call_id: "call-read-4".to_string(),
                tool_id: read_tool_id.clone(),
                arguments: serde_json::json!({"path": workspace.join("file_4.txt").to_str().unwrap()}),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::ToolCalls,
            },
        ],
        vec![
            ProviderStreamEvent::TextDelta {
                text: "all files read successfully".to_string(),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::Stop,
            },
        ],
    ]);

    let config = Config::default();
    let active_tools = std::sync::Arc::new(ActiveToolSet::builtins(&config, &workspace).unwrap());
    let neutral_executor = active_tools.neutral_executor(rig::tool::ToolContext::new());
    let cancellation = CancellationSignal::default();

    let request = NeutralTurnRequest {
        model: "fixture".to_string(),
        messages: vec![ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: "read all files".to_string(),
            }],
        }],
        credential: None,
        max_output_tokens: None,
        tools: vec![ProviderToolDefinition {
            id: read_tool_id,
            description: "read".to_string(),
            argument_schema: serde_json::json!({"type":"object"}),
        }],
        max_turns: 3,
        checkpoint: None,
    };

    let terminal = run_neutral_turn(
        NeutralTurnRuntime {
            provider: &provider,
            tools: &neutral_executor,
            observer: &NoopTurnObserver,
            cancellation: &cancellation,
            steering: &NoopSteeringQueue,
        },
        request,
    )
    .await
    .unwrap();

    let NeutralTurnTerminal::Completed(output) = terminal else {
        panic!("expected completed turn");
    };

    assert_eq!(output.tool_calls, 4);
    assert_eq!(output.text, "all files read successfully");

    // Verify ordering of tool results in the messages sent to the 2nd turn
    let second_turn_messages = &provider.requests.lock().unwrap()[1].messages;
    let tool_result_ids: Vec<&str> = second_turn_messages
        .iter()
        .filter(|m| m.role == MessageRole::Tool)
        .flat_map(|m| {
            m.content.iter().filter_map(|c| match c {
                MessageContent::ToolResult { call_id, .. } => Some(call_id.as_str()),
                _ => None,
            })
        })
        .collect();

    assert_eq!(
        tool_result_ids,
        vec!["call-read-1", "call-read-2", "call-read-3", "call-read-4"]
    );

    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn mixed_batch_with_write_falls_back_to_sequential() {
    let workspace = temp_workspace();
    std::fs::write(workspace.join("sample.txt"), "initial content").unwrap();

    let read_tool_id: CapabilityId = "tool:read".parse().unwrap();
    let write_tool_id: CapabilityId = "tool:write".parse().unwrap();

    let provider = FixtureProvider::new([
        vec![
            ProviderStreamEvent::ToolCall {
                call_id: "call-1-read".to_string(),
                tool_id: read_tool_id.clone(),
                arguments: serde_json::json!({"path": workspace.join("sample.txt").to_str().unwrap()}),
            },
            ProviderStreamEvent::ToolCall {
                call_id: "call-2-write".to_string(),
                tool_id: write_tool_id.clone(),
                arguments: serde_json::json!({
                    "path": workspace.join("output.txt").to_str().unwrap(),
                    "content": "written content"
                }),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::ToolCalls,
            },
        ],
        vec![
            ProviderStreamEvent::TextDelta {
                text: "done writing".to_string(),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::Stop,
            },
        ],
    ]);

    let config = Config {
        auto_approve: true,
        ..Config::default()
    };
    let active_tools = std::sync::Arc::new(ActiveToolSet::builtins(&config, &workspace).unwrap());
    let approval_cap = ApprovalCapability::new(true, Arc::new(TestApprovalSink));
    let neutral_executor = active_tools.neutral_executor(approval_context(approval_cap));
    let cancellation = CancellationSignal::default();

    let request = NeutralTurnRequest {
        model: "fixture".to_string(),
        messages: vec![ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: "read and write".to_string(),
            }],
        }],
        credential: None,
        max_output_tokens: None,
        tools: vec![
            ProviderToolDefinition {
                id: read_tool_id,
                description: "read".to_string(),
                argument_schema: serde_json::json!({"type":"object"}),
            },
            ProviderToolDefinition {
                id: write_tool_id,
                description: "write".to_string(),
                argument_schema: serde_json::json!({"type":"object"}),
            },
        ],
        max_turns: 3,
        checkpoint: None,
    };

    let terminal = run_neutral_turn(
        NeutralTurnRuntime {
            provider: &provider,
            tools: &neutral_executor,
            observer: &NoopTurnObserver,
            cancellation: &cancellation,
            steering: &NoopSteeringQueue,
        },
        request,
    )
    .await
    .unwrap();

    assert!(matches!(terminal, NeutralTurnTerminal::Completed(_)));
    assert!(workspace.join("output.txt").exists());
    assert_eq!(
        std::fs::read_to_string(workspace.join("output.txt")).unwrap(),
        "written content"
    );

    let _ = std::fs::remove_dir_all(workspace);
}
