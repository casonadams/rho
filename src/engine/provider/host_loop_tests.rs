use super::*;
use crate::plugin::contract::{AuthenticationRequest, AuthenticationResponse, ProviderDescriptor, ProviderStreamEvent};
use futures::stream::{self, BoxStream};
use std::collections::VecDeque;
use std::sync::Mutex;

struct FixtureProvider {
    turns: Mutex<VecDeque<Vec<Result<ProviderStreamEvent, CapabilityError>>>>,
    requests: Arc<Mutex<Vec<ProviderRequest>>>,
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
            requests: Arc::new(Mutex::new(Vec::new())),
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
struct FixtureTools {
    calls: Mutex<Vec<NeutralToolCall>>,
}

#[async_trait]
impl NeutralToolExecutor for FixtureTools {
    async fn execute(&self, call: NeutralToolCall) -> Result<NeutralToolResult, NeutralTurnError> {
        self.calls.lock().unwrap().push(call);
        Ok(NeutralToolResult {
            content: "tool result".to_string(),
            is_error: false,
        })
    }
}

fn cancellation() -> &'static CancellationSignal {
    static SIGNAL: std::sync::OnceLock<CancellationSignal> = std::sync::OnceLock::new();
    SIGNAL.get_or_init(CancellationSignal::default)
}

fn runtime<'a>(provider: &'a FixtureProvider, tools: &'a dyn NeutralToolExecutor) -> NeutralTurnRuntime<'a> {
    NeutralTurnRuntime {
        provider,
        tools,
        observer: &NoopTurnObserver,
        cancellation: cancellation(),
    }
}

fn request(max_turns: usize) -> NeutralTurnRequest {
    NeutralTurnRequest {
        model: "fixture".to_string(),
        messages: vec![ModelMessage {
            role: MessageRole::User,
            content: vec![MessageContent::Text {
                text: "hello".to_string(),
            }],
        }],
        credential: None,
        max_output_tokens: None,
        tools: vec![ProviderToolDefinition {
            id: "tool:fixture".parse().unwrap(),
            description: "fixture".to_string(),
            argument_schema: serde_json::json!({"type":"object"}),
        }],
        max_turns,
        checkpoint: None,
    }
}

#[tokio::test]
async fn transcript_orders_text_tool_call_result_and_terminal_text() {
    let provider = FixtureProvider::new([
        vec![
            ProviderStreamEvent::TextDelta {
                text: "checking".to_string(),
            },
            ProviderStreamEvent::ToolCall {
                call_id: "call-1".to_string(),
                tool_id: "tool:fixture".parse().unwrap(),
                arguments: serde_json::json!({"value":1}),
            },
            ProviderStreamEvent::Usage {
                input_tokens: 3,
                output_tokens: 2,
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::ToolCalls,
            },
        ],
        vec![
            ProviderStreamEvent::TextDelta {
                text: "done".to_string(),
            },
            ProviderStreamEvent::Usage {
                input_tokens: 5,
                output_tokens: 1,
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::Stop,
            },
        ],
    ]);
    let tools = FixtureTools::default();
    let terminal = run_neutral_turn(runtime(&provider, &tools), request(3)).await.unwrap();
    let NeutralTurnTerminal::Completed(output) = terminal else {
        panic!("expected completion")
    };
    assert_eq!(output.text, "checkingdone");
    assert_eq!(output.usage.input_tokens, 8);
    assert_eq!(output.tool_calls, 1);
    assert_eq!(provider.requests.lock().unwrap()[1].messages.len(), 3);
    assert!(matches!(
        provider.requests.lock().unwrap()[1].messages[2].content[0],
        MessageContent::ToolResult { ref call_id, .. } if call_id == "call-1"
    ));
}

#[tokio::test]
async fn malformed_and_nonterminal_streams_fail_closed() {
    let malformed = FixtureProvider::new([vec![ProviderStreamEvent::ToolCallDelta {
        call_id: "call-1".to_string(),
        tool_id: "tool:fixture".parse().unwrap(),
        arguments_delta: "{".to_string(),
    }]]);
    let tools = FixtureTools::default();
    let error = run_neutral_turn(runtime(&malformed, &tools), request(1))
        .await
        .unwrap_err();
    assert!(matches!(error, NeutralTurnError::Malformed(_)));

    let post_terminal = FixtureProvider::new([vec![
        ProviderStreamEvent::Finished {
            reason: FinishReason::Stop,
        },
        ProviderStreamEvent::TextDelta {
            text: "late".to_string(),
        },
    ]]);
    assert!(matches!(
        run_neutral_turn(runtime(&post_terminal, &tools), request(1),).await,
        Err(NeutralTurnError::Malformed(_))
    ));
}

#[tokio::test]
async fn budget_checkpoint_resumes_without_replaying_completed_tools() {
    let first = FixtureProvider::new([vec![
        ProviderStreamEvent::ToolCall {
            call_id: "call-1".to_string(),
            tool_id: "tool:fixture".parse().unwrap(),
            arguments: serde_json::json!({}),
        },
        ProviderStreamEvent::Finished {
            reason: FinishReason::ToolCalls,
        },
    ]]);
    let tools = FixtureTools::default();
    let terminal = run_neutral_turn(runtime(&first, &tools), request(1)).await.unwrap();
    let NeutralTurnTerminal::Checkpoint(checkpoint) = terminal else {
        panic!("expected checkpoint")
    };
    assert_eq!(tools.calls.lock().unwrap().len(), 1);

    let second = FixtureProvider::new([vec![
        ProviderStreamEvent::TextDelta {
            text: "continued".to_string(),
        },
        ProviderStreamEvent::Finished {
            reason: FinishReason::Stop,
        },
    ]]);
    let mut resumed = request(3);
    resumed.messages.clear();
    resumed.checkpoint = Some(checkpoint);
    let terminal = run_neutral_turn(runtime(&second, &tools), resumed).await.unwrap();
    assert!(matches!(terminal, NeutralTurnTerminal::Completed(_)));
    assert_eq!(tools.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn cancellation_returns_a_durable_continuation_boundary() {
    let provider = FixtureProvider::new(std::iter::empty::<Vec<ProviderStreamEvent>>());
    let cancellation = CancellationSignal::default();
    cancellation.cancel();
    let terminal = run_neutral_turn(
        NeutralTurnRuntime {
            provider: &provider,
            tools: &FixtureTools::default(),
            observer: &NoopTurnObserver,
            cancellation: &cancellation,
        },
        request(2),
    )
    .await
    .unwrap();
    let NeutralTurnTerminal::Cancelled(checkpoint) = terminal else {
        panic!("expected cancellation")
    };
    assert_eq!(checkpoint.messages.len(), 1);
    assert!(provider.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unknown_tool_and_duplicate_call_identifiers_are_rejected() {
    for events in [
        vec![
            ProviderStreamEvent::ToolCall {
                call_id: "call".to_string(),
                tool_id: "tool:unknown".parse().unwrap(),
                arguments: serde_json::json!({}),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::ToolCalls,
            },
        ],
        vec![
            ProviderStreamEvent::ToolCall {
                call_id: "call".to_string(),
                tool_id: "tool:fixture".parse().unwrap(),
                arguments: serde_json::json!({}),
            },
            ProviderStreamEvent::ToolCall {
                call_id: "call".to_string(),
                tool_id: "tool:fixture".parse().unwrap(),
                arguments: serde_json::json!({}),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::ToolCalls,
            },
        ],
    ] {
        let provider = FixtureProvider::new([events]);
        assert!(
            run_neutral_turn(runtime(&provider, &FixtureTools::default()), request(1),)
                .await
                .is_err()
        );
    }
}
