#![cfg(unix)]

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use rho::engine::provider::host_loop::{
    CancellationSignal, NeutralTurnRequest, NeutralTurnRuntime, NeutralTurnTerminal, NoopTurnObserver,
    SteeringQueueProvider, run_neutral_turn,
};
use rho::plugin::capability::{CapabilityError, CapabilityId};
use rho::plugin::contract::{
    AuthenticationRequest, AuthenticationResponse, ExecutionMode, FinishReason, MessageContent, MessageRole,
    ModelMessage, ProviderCapability, ProviderDescriptor, ProviderRequest, ProviderStreamEvent, ProviderToolDefinition,
};
use std::collections::VecDeque;
use std::sync::Mutex;

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

struct StreamFixtureProvider {
    first_stream: Mutex<Option<BoxStream<'static, Result<ProviderStreamEvent, CapabilityError>>>>,
    second_turn: Vec<ProviderStreamEvent>,
    requests: Mutex<Vec<ProviderRequest>>,
}

impl StreamFixtureProvider {
    fn new(
        first_stream: BoxStream<'static, Result<ProviderStreamEvent, CapabilityError>>,
        second_turn: Vec<ProviderStreamEvent>,
    ) -> Self {
        Self {
            first_stream: Mutex::new(Some(first_stream)),
            second_turn,
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ProviderCapability for StreamFixtureProvider {
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
        if let Some(stream) = self.first_stream.lock().unwrap().take() {
            Ok(stream)
        } else {
            let turn: Vec<Result<ProviderStreamEvent, CapabilityError>> =
                self.second_turn.clone().into_iter().map(Ok).collect();
            Ok(Box::pin(stream::iter(turn)))
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

#[derive(Default)]
struct MockSteeringQueue {
    queue: Mutex<VecDeque<String>>,
}

impl MockSteeringQueue {
    fn enqueue(&self, msg: &str) {
        self.queue.lock().unwrap().push_back(msg.to_string());
    }
}

#[async_trait]
impl SteeringQueueProvider for MockSteeringQueue {
    async fn poll_steering(&self) -> Vec<String> {
        self.queue.lock().unwrap().drain(..).collect()
    }
}

#[derive(Default)]
struct DummyTools;

#[async_trait]
impl rho::engine::provider::host_loop::NeutralToolExecutor for DummyTools {
    fn execution_mode(&self, _tool_id: &CapabilityId) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    async fn execute(
        &self,
        call: rho::engine::provider::host_loop::NeutralToolCall,
    ) -> Result<rho::engine::provider::host_loop::NeutralToolResult, rho::engine::provider::host_loop::NeutralTurnError>
    {
        Ok(rho::engine::provider::host_loop::NeutralToolResult {
            content: format!("executed {}", call.call_id),
            is_error: false,
        })
    }
}

#[tokio::test]
async fn test_mid_generation_stream_interruption_and_steer_injection() {
    let (turn1_tx, turn1_rx) = futures::channel::mpsc::unbounded();
    // Send partial chunk
    turn1_tx
        .unbounded_send(Ok(ProviderStreamEvent::TextDelta {
            text: "partial text before steer".to_string(),
        }))
        .unwrap();

    let provider = StreamFixtureProvider::new(
        Box::pin(turn1_rx),
        vec![
            ProviderStreamEvent::TextDelta {
                text: " redirected answer".to_string(),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::Stop,
            },
        ],
    );

    let cancellation = CancellationSignal::default();
    let steering = MockSteeringQueue::default();

    // Trigger steer after chunk is received
    let cancel_clone = cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
        cancel_clone.interrupt_stream();
    });
    steering.enqueue("stop and redirect here");

    let tools = DummyTools;
    let request = NeutralTurnRequest {
        model: "fixture".to_string(),
        messages: vec![ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: "original prompt".to_string(),
            }],
        }],
        credential: None,
        max_output_tokens: None,
        tools: Vec::new(),
        max_turns: 3,
        checkpoint: None,
    };

    let terminal = run_neutral_turn(
        NeutralTurnRuntime {
            provider: &provider,
            tools: &tools,
            observer: &NoopTurnObserver,
            cancellation: &cancellation,
            steering: &steering,
        },
        request,
    )
    .await
    .unwrap();

    let NeutralTurnTerminal::Completed(output) = terminal else {
        panic!("expected completion after steer");
    };

    assert_eq!(output.model_turns, 2);
    assert_eq!(output.text, "partial text before steer redirected answer");

    // Check second turn request messages
    let reqs = provider.requests.lock().unwrap();
    assert_eq!(reqs.len(), 2);
    let second_req_messages = &reqs[1].messages;
    assert_eq!(second_req_messages.len(), 3);
    assert!(matches!(&second_req_messages[0].content[0], MessageContent::Text { text } if text == "original prompt"));
    assert!(
        matches!(&second_req_messages[1].content[0], MessageContent::Text { text } if text == "partial text before steer")
    );
    assert!(
        matches!(&second_req_messages[2].content[0], MessageContent::Text { text } if text == "stop and redirect here")
    );
}

#[tokio::test]
async fn test_tool_boundary_steering_injection() {
    let tool_id: CapabilityId = "tool:dummy".parse().unwrap();
    let provider = FixtureProvider::new([
        vec![
            ProviderStreamEvent::ToolCall {
                call_id: "call-1".to_string(),
                tool_id: tool_id.clone(),
                arguments: serde_json::json!({}),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::ToolCalls,
            },
        ],
        vec![
            ProviderStreamEvent::TextDelta {
                text: "final answer with steer context".to_string(),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::Stop,
            },
        ],
    ]);

    let cancellation = CancellationSignal::default();
    let steering = MockSteeringQueue::default();
    steering.enqueue("steering instruction queued during tool");

    let tools = DummyTools;
    let request = NeutralTurnRequest {
        model: "fixture".to_string(),
        messages: vec![ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: "do work".to_string(),
            }],
        }],
        credential: None,
        max_output_tokens: None,
        tools: vec![ProviderToolDefinition {
            id: tool_id,
            description: "dummy".to_string(),
            argument_schema: serde_json::json!({"type":"object"}),
        }],
        max_turns: 3,
        checkpoint: None,
    };

    let terminal = run_neutral_turn(
        NeutralTurnRuntime {
            provider: &provider,
            tools: &tools,
            observer: &NoopTurnObserver,
            cancellation: &cancellation,
            steering: &steering,
        },
        request,
    )
    .await
    .unwrap();

    let NeutralTurnTerminal::Completed(output) = terminal else {
        panic!("expected completion");
    };

    assert_eq!(output.tool_calls, 1);
    assert_eq!(output.model_turns, 2);

    // Verify turn 2 received user prompt, assistant tool call, tool result, AND steering prompt
    let reqs = provider.requests.lock().unwrap();
    let turn2_messages = &reqs[1].messages;
    assert_eq!(turn2_messages.len(), 4);
    assert_eq!(turn2_messages[0].role, MessageRole::User);
    assert_eq!(turn2_messages[1].role, MessageRole::Assistant);
    assert_eq!(turn2_messages[2].role, MessageRole::Tool);
    assert_eq!(turn2_messages[3].role, MessageRole::User);
    assert!(
        matches!(&turn2_messages[3].content[0], MessageContent::Text { text } if text == "steering instruction queued during tool")
    );
}
