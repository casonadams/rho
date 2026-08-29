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

#[derive(Default)]
struct ConfigurableFixtureTools {
    calls: Mutex<Vec<NeutralToolCall>>,
    mode: Mutex<std::collections::HashMap<CapabilityId, ExecutionMode>>,
    delay_ms: Mutex<std::collections::HashMap<String, u64>>,
    concurrency_tracker: Arc<std::sync::atomic::AtomicUsize>,
    max_observed_concurrency: Arc<std::sync::atomic::AtomicUsize>,
}

impl ConfigurableFixtureTools {
    fn set_mode(&self, tool_id: CapabilityId, mode: ExecutionMode) {
        self.mode.lock().unwrap().insert(tool_id, mode);
    }

    fn set_delay(&self, call_id: &str, delay: u64) {
        self.delay_ms.lock().unwrap().insert(call_id.to_string(), delay);
    }
}

#[async_trait]
impl NeutralToolExecutor for ConfigurableFixtureTools {
    fn execution_mode(&self, tool_id: &CapabilityId) -> ExecutionMode {
        self.mode
            .lock()
            .unwrap()
            .get(tool_id)
            .copied()
            .unwrap_or(ExecutionMode::Sequential)
    }

    async fn execute(&self, call: NeutralToolCall) -> Result<NeutralToolResult, NeutralTurnError> {
        let current = self
            .concurrency_tracker
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        self.max_observed_concurrency
            .fetch_max(current, std::sync::atomic::Ordering::SeqCst);

        let delay = self.delay_ms.lock().unwrap().get(&call.call_id).copied().unwrap_or(0);
        if delay > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
        }

        self.calls.lock().unwrap().push(call.clone());
        self.concurrency_tracker
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);

        Ok(NeutralToolResult {
            content: format!("result for {}", call.call_id),
            is_error: false,
        })
    }
}

#[tokio::test]
async fn parallel_tools_execute_concurrently_and_preserve_emission_order() {
    let tool_id: CapabilityId = "tool:fixture".parse().unwrap();
    let provider = FixtureProvider::new([
        vec![
            ProviderStreamEvent::ToolCall {
                call_id: "call-1".to_string(),
                tool_id: tool_id.clone(),
                arguments: serde_json::json!({"n": 1}),
            },
            ProviderStreamEvent::ToolCall {
                call_id: "call-2".to_string(),
                tool_id: tool_id.clone(),
                arguments: serde_json::json!({"n": 2}),
            },
            ProviderStreamEvent::ToolCall {
                call_id: "call-3".to_string(),
                tool_id: tool_id.clone(),
                arguments: serde_json::json!({"n": 3}),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::ToolCalls,
            },
        ],
        vec![
            ProviderStreamEvent::TextDelta {
                text: "done".to_string(),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::Stop,
            },
        ],
    ]);

    let tools = ConfigurableFixtureTools::default();
    tools.set_mode(tool_id, ExecutionMode::Parallel);
    // call-1 finishes last (60ms), call-3 finishes first (10ms)
    tools.set_delay("call-1", 60);
    tools.set_delay("call-2", 30);
    tools.set_delay("call-3", 10);

    let start = std::time::Instant::now();
    let terminal = run_neutral_turn(runtime(&provider, &tools), request(3)).await.unwrap();
    let elapsed = start.elapsed();

    assert!(matches!(terminal, NeutralTurnTerminal::Completed(_)));
    // If sequential: 60 + 30 + 10 = 100ms. If concurrent: max(60, 30, 10) ~= 60ms.
    assert!(elapsed.as_millis() < 95);
    assert!(tools.max_observed_concurrency.load(std::sync::atomic::Ordering::SeqCst) >= 2);

    // Verify messages in provider request are strictly ordered call-1, call-2, call-3
    let second_turn_req = &provider.requests.lock().unwrap()[1];
    let tool_results: Vec<&str> = second_turn_req
        .messages
        .iter()
        .filter(|m| m.role == MessageRole::Tool)
        .flat_map(|m| {
            m.content.iter().filter_map(|c| match c {
                MessageContent::ToolResult { call_id, .. } => Some(call_id.as_str()),
                _ => None,
            })
        })
        .collect();

    assert_eq!(tool_results, vec!["call-1", "call-2", "call-3"]);
}

#[tokio::test]
async fn mixed_tool_batch_executes_sequentially() {
    let parallel_id: CapabilityId = "tool:parallel".parse().unwrap();
    let sequential_id: CapabilityId = "tool:sequential".parse().unwrap();

    let provider = FixtureProvider::new([
        vec![
            ProviderStreamEvent::ToolCall {
                call_id: "call-1".to_string(),
                tool_id: parallel_id.clone(),
                arguments: serde_json::json!({}),
            },
            ProviderStreamEvent::ToolCall {
                call_id: "call-2".to_string(),
                tool_id: sequential_id.clone(),
                arguments: serde_json::json!({}),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::ToolCalls,
            },
        ],
        vec![
            ProviderStreamEvent::TextDelta {
                text: "done".to_string(),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::Stop,
            },
        ],
    ]);

    let mut req = request(3);
    req.tools = vec![
        ProviderToolDefinition {
            id: parallel_id.clone(),
            description: "p".to_string(),
            argument_schema: serde_json::json!({"type":"object"}),
        },
        ProviderToolDefinition {
            id: sequential_id.clone(),
            description: "s".to_string(),
            argument_schema: serde_json::json!({"type":"object"}),
        },
    ];

    let tools = ConfigurableFixtureTools::default();
    tools.set_mode(parallel_id, ExecutionMode::Parallel);
    tools.set_mode(sequential_id, ExecutionMode::Sequential);
    tools.set_delay("call-1", 20);
    tools.set_delay("call-2", 20);

    let terminal = run_neutral_turn(runtime(&provider, &tools), req).await.unwrap();
    assert!(matches!(terminal, NeutralTurnTerminal::Completed(_)));

    // Concurrency must remain 1 for mixed batch
    assert_eq!(
        tools.max_observed_concurrency.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn parallel_tools_concurrency_is_bounded_to_eight() {
    let tool_id: CapabilityId = "tool:fixture".parse().unwrap();
    let calls: Vec<ProviderStreamEvent> = (0..12)
        .map(|i| ProviderStreamEvent::ToolCall {
            call_id: format!("call-{i}"),
            tool_id: tool_id.clone(),
            arguments: serde_json::json!({}),
        })
        .chain(std::iter::once(ProviderStreamEvent::Finished {
            reason: FinishReason::ToolCalls,
        }))
        .collect();

    let provider = FixtureProvider::new([
        calls,
        vec![
            ProviderStreamEvent::TextDelta {
                text: "done".to_string(),
            },
            ProviderStreamEvent::Finished {
                reason: FinishReason::Stop,
            },
        ],
    ]);

    let tools = ConfigurableFixtureTools::default();
    tools.set_mode(tool_id, ExecutionMode::Parallel);
    for i in 0..12 {
        tools.set_delay(&format!("call-{i}"), 15);
    }

    let terminal = run_neutral_turn(runtime(&provider, &tools), request(3)).await.unwrap();
    assert!(matches!(terminal, NeutralTurnTerminal::Completed(_)));

    let max_concurrency = tools.max_observed_concurrency.load(std::sync::atomic::Ordering::SeqCst);
    assert!(max_concurrency <= 8, "concurrency was {max_concurrency}, expected <= 8");
    assert!(max_concurrency >= 2);
}

#[tokio::test]
async fn cancellation_during_parallel_execution_aborts_cleanly() {
    let tool_id: CapabilityId = "tool:fixture".parse().unwrap();
    let provider = FixtureProvider::new([vec![
        ProviderStreamEvent::ToolCall {
            call_id: "call-1".to_string(),
            tool_id: tool_id.clone(),
            arguments: serde_json::json!({}),
        },
        ProviderStreamEvent::ToolCall {
            call_id: "call-2".to_string(),
            tool_id: tool_id.clone(),
            arguments: serde_json::json!({}),
        },
        ProviderStreamEvent::Finished {
            reason: FinishReason::ToolCalls,
        },
    ]]);

    let cancellation = CancellationSignal::default();
    let tools = ConfigurableFixtureTools::default();
    tools.set_mode(tool_id, ExecutionMode::Parallel);
    tools.set_delay("call-1", 100);
    tools.set_delay("call-2", 100);

    let cancel_sig = cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(15)).await;
        cancel_sig.cancel();
    });

    let terminal = run_neutral_turn(
        NeutralTurnRuntime {
            provider: &provider,
            tools: &tools,
            observer: &NoopTurnObserver,
            cancellation: &cancellation,
            steering: &NoopSteeringQueue,
        },
        request(2),
    )
    .await
    .unwrap();

    assert!(matches!(terminal, NeutralTurnTerminal::Cancelled(_)));
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
        steering: &NoopSteeringQueue,
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
            steering: &NoopSteeringQueue,
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
