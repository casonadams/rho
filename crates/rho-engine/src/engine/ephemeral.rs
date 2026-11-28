//! Cache-preserving ephemeral model turns (`/btw`, `<recap>`) that read
//! canonical history without mutating session state or dispatching tools.

use futures::StreamExt;
use rho_harness_core::args::{TodoItem, TodoStatus};
use rho_harness_core::error::{AppError, Result};
use rho_harness_core::model::ChatMessage;
use tokio::sync::mpsc;

use super::AgentEngine;
use super::metrics::StructuralUsage;
use super::runner::history::{continuation_history, without_unanswered_tool_calls};
use super::runner::turn::runner::tool_definitions;
use super::runner::turn::types::{CancellationSignal, unless_cancelled};
use crate::provider::adapter::{ModelCompletionRequest, ModelEventStream, ModelHandle, ModelStreamEvent};

const MAX_RECAP_CHARS: usize = 280;
const RECAP_MAX_TOKENS: u64 = 160;
const EPHEMERAL_CANCELLED_MSG: &str = "ephemeral turn cancelled";

/// Request parameters for a single non-mutating, cache-preserving ephemeral turn.
#[derive(Debug, Clone, Default)]
pub struct EphemeralTurnRequest {
    pub prompt: String,
    pub extra_messages: Vec<ChatMessage>,
    pub partial_assistant: Option<String>,
    pub max_tokens: Option<u64>,
    pub cancellation: CancellationSignal,
}

/// Output collected from an ephemeral turn.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EphemeralTurnResponse {
    pub text: String,
    pub usage: Option<StructuralUsage>,
}

/// A self-contained, `'static` ephemeral turn snapshot ready to execute on a
/// background task without borrowing `AgentEngine`.
pub struct PreparedEphemeralTurn {
    model: ModelHandle,
    request: ModelCompletionRequest,
    cancellation: CancellationSignal,
}

impl PreparedEphemeralTurn {
    /// Streams the prepared ephemeral completion, optionally forwarding text
    /// deltas to `delta_tx`.
    pub async fn execute(self, delta_tx: Option<mpsc::UnboundedSender<String>>) -> Result<EphemeralTurnResponse> {
        if self.cancellation.is_cancelled() {
            return Err(AppError::Cancelled(EPHEMERAL_CANCELLED_MSG.to_string()));
        }
        let stream = unless_cancelled(Some(&self.cancellation), self.model.stream(self.request))
            .await
            .ok_or_else(|| AppError::Cancelled(EPHEMERAL_CANCELLED_MSG.to_string()))??;
        collect_ephemeral_stream(stream, &self.cancellation, delta_tx).await
    }
}

impl AgentEngine {
    /// Snapshots canonical history, system prompt, and tools into a `'static`
    /// `PreparedEphemeralTurn` that can be spawned on a background task.
    pub async fn prepare_ephemeral_turn(&self, request: EphemeralTurnRequest) -> Result<PreparedEphemeralTurn> {
        let history = self.snapshot_history().await?;
        self.prepare_ephemeral_with_history(history, request).await
    }

    /// Snapshots session history into a `'static` `<recap>` turn, or `Ok(None)`
    /// when the session has no model-visible history yet.
    pub async fn prepare_recap(&self, cancellation: CancellationSignal) -> Result<Option<PreparedEphemeralTurn>> {
        let history = self.snapshot_history().await?;
        if history.is_empty() {
            return Ok(None);
        }
        let session_name = self.session_manager.cached_session_name();
        let todos = rho_harness_core::todo::latest_todos(&history);
        let prompt = build_recap_prompt(session_name.as_deref(), todos.as_deref());
        let prepared = self
            .prepare_ephemeral_with_history(
                history,
                EphemeralTurnRequest {
                    prompt,
                    max_tokens: Some(RECAP_MAX_TOKENS),
                    cancellation,
                    ..Default::default()
                },
            )
            .await?;
        Ok(Some(prepared))
    }

    /// Runs a single-shot ephemeral completion against the snapshotted session
    /// history, preserving the provider prompt cache prefix without mutating
    /// session state, turn notes, usage trackers, or dispatching tool calls.
    pub async fn run_ephemeral_turn(
        &self,
        request: EphemeralTurnRequest,
        delta_tx: Option<mpsc::UnboundedSender<String>>,
    ) -> Result<EphemeralTurnResponse> {
        self.prepare_ephemeral_turn(request).await?.execute(delta_tx).await
    }

    /// Generates a short `<recap>` summary of the current session, or `Ok(None)`
    /// when the session has no model-visible history yet.
    pub async fn generate_recap(&self, cancellation: CancellationSignal) -> Result<Option<String>> {
        let Some(prepared) = self.prepare_recap(cancellation).await? else {
            return Ok(None);
        };
        let response = prepared.execute(None).await?;
        Ok(format_recap_text(&response.text))
    }

    async fn snapshot_history(&self) -> Result<Vec<ChatMessage>> {
        let loaded = self.load_initial_history().await?;
        let checkpoint = self.session_manager.load_checkpoint().await?;
        let combined = match checkpoint.as_deref() {
            Some(pending) => continuation_history(&loaded.messages, pending),
            None => loaded.messages,
        };
        Ok(without_unanswered_tool_calls(&combined).to_vec())
    }

    async fn prepare_ephemeral_with_history(
        &self,
        mut messages: Vec<ChatMessage>,
        request: EphemeralTurnRequest,
    ) -> Result<PreparedEphemeralTurn> {
        if request.cancellation.is_cancelled() {
            return Err(AppError::Cancelled(EPHEMERAL_CANCELLED_MSG.to_string()));
        }
        let model = self.require_model()?.clone();
        let preamble = self.build_ephemeral_preamble().await?;
        let tools_guard = self.tools.read().await;
        let tools = tool_definitions(&tools_guard);
        drop(tools_guard);

        append_ephemeral_messages(
            &mut messages,
            request.partial_assistant.as_deref(),
            &request.extra_messages,
            &request.prompt,
        );

        let completion_req = ModelCompletionRequest {
            preamble: (!preamble.is_empty()).then_some(preamble),
            prompt: String::new(),
            messages: messages.into(),
            tools,
            max_tokens: request.max_tokens,
            temperature: None,
            thinking_level: None,
        };

        Ok(PreparedEphemeralTurn {
            model,
            request: completion_req,
            cancellation: request.cancellation,
        })
    }

    async fn build_ephemeral_preamble(&self) -> Result<String> {
        let context = self.project_context().await?;
        let mut preamble = context.build_system_prompt();
        crate::mcp::append_server_instructions(&mut preamble, &self.mcp.instructions);
        Ok(preamble)
    }
}

async fn collect_ephemeral_stream(
    mut stream: ModelEventStream,
    cancellation: &CancellationSignal,
    delta_tx: Option<mpsc::UnboundedSender<String>>,
) -> Result<EphemeralTurnResponse> {
    let mut text = String::new();
    let mut usage: Option<StructuralUsage> = None;

    while let Some(item) = unless_cancelled(Some(cancellation), stream.next())
        .await
        .ok_or_else(|| AppError::Cancelled(EPHEMERAL_CANCELLED_MSG.to_string()))?
    {
        match item? {
            ModelStreamEvent::Text(delta) => {
                text.push_str(&delta);
                if let Some(tx) = &delta_tx {
                    let _ = tx.send(delta);
                }
            }
            ModelStreamEvent::Usage(u) => {
                usage = Some(usage.map_or(u, |prev| prev.merge(&u)));
            }
            ModelStreamEvent::Reasoning(_)
            | ModelStreamEvent::ReasoningBlock(_)
            | ModelStreamEvent::ToolInputDelta { .. }
            | ModelStreamEvent::ToolCall(_)
            | ModelStreamEvent::Finish(_) => {}
        }
    }

    Ok(EphemeralTurnResponse { text, usage })
}

fn append_ephemeral_messages(
    messages: &mut Vec<ChatMessage>,
    partial_assistant: Option<&str>,
    extra_messages: &[ChatMessage],
    prompt: &str,
) {
    let partial = partial_assistant.map(str::trim).filter(|s| !s.is_empty());
    let mut folded_prefix = None;

    if let Some(partial_text) = partial {
        if matches!(messages.last(), Some(ChatMessage::User { .. })) {
            messages.push(ChatMessage::assistant(partial_text));
        } else {
            folded_prefix = Some(partial_text);
        }
    }

    messages.extend_from_slice(extra_messages);

    let final_prompt = match folded_prefix {
        Some(prefix) => format!("[In-progress assistant response so far:\n{prefix}]\n\n{prompt}"),
        None => prompt.to_string(),
    };
    messages.push(ChatMessage::user(final_prompt));
}

fn next_actionable_todo(todos: &[TodoItem]) -> Option<&TodoItem> {
    todos
        .iter()
        .find(|item| item.status == TodoStatus::InProgress)
        .or_else(|| todos.iter().find(|item| item.status == TodoStatus::Pending))
}

/// Builds the `<recap>` prompt, optionally anchored with the session title and
/// the next actionable todo item.
pub fn build_recap_prompt(session_name: Option<&str>, todos: Option<&[TodoItem]>) -> String {
    let mut lines = vec![
        "<recap>".to_string(),
        "Summarize where this session stands right now in under 40 words (1-2 plain sentences, no markdown, no tool calls).".to_string(),
        "Lead with the overall goal and current task, then state one concrete next action; skip root-cause narratives and tangents.".to_string(),
    ];
    if let Some(name) = session_name.map(str::trim).filter(|s| !s.is_empty()) {
        lines.push(format!("Session title: {name}"));
    }
    if let Some(next_todo) = todos.and_then(next_actionable_todo) {
        lines.push(format!("Next todo: {}", next_todo.content));
    }
    lines.push("</recap>".to_string());
    lines.join("\n")
}

fn strip_matching_quotes(text: &str) -> &str {
    let bytes = text.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        return text[1..text.len() - 1].trim();
    }
    text
}

/// Sanitizes and caps raw `<recap>` model output for inline UI display.
pub fn format_recap_text(raw: &str) -> Option<String> {
    let without_tags = raw.replace("<recap>", "").replace("</recap>", "");
    let collapsed = without_tags.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = strip_matching_quotes(collapsed.trim());
    if cleaned.is_empty() {
        return None;
    }
    if cleaned.chars().count() <= MAX_RECAP_CHARS {
        return Some(cleaned.to_string());
    }
    let cutoff = cleaned
        .char_indices()
        .nth(MAX_RECAP_CHARS - 1)
        .map_or(cleaned.len(), |(idx, _)| idx);
    Some(format!("{}…", cleaned[..cutoff].trim_end()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::eval::mock::mock_engine_in_dir;
    use crate::engine::runner::TurnRequest;
    use crate::provider::adapter::{FinishReason, MockAdapter, ModelAdapter, ModelStreamEvent};
    use async_trait::async_trait;
    use rho_harness_core::model::{ToolCall, ToolFunction};
    use std::sync::Arc;

    #[tokio::test]
    async fn ephemeral_turn_preserves_preamble_and_tools_and_does_not_mutate_session_or_dispatch_tools() {
        let model = Arc::new(MockAdapter::default());
        model.add_turn(vec![Ok(ModelStreamEvent::Text("main reply".to_string()))]);
        model.add_turn(vec![
            Ok(ModelStreamEvent::Text("btw ".to_string())),
            Ok(ModelStreamEvent::ToolCall(ToolCall::new(
                "call-1",
                ToolFunction::new("bash", serde_json::json!({ "command": "echo should-not-run" })),
            ))),
            Ok(ModelStreamEvent::Text("answer".to_string())),
            Ok(ModelStreamEvent::Usage(StructuralUsage {
                input_tokens: 100,
                output_tokens: 12,
                cached_input_tokens: Some(80),
                ..StructuralUsage::default()
            })),
            Ok(ModelStreamEvent::Finish(FinishReason::Stop)),
        ]);

        let (engine, _dir) = mock_engine_in_dir("ephemeral-cache", model.clone());
        let presenter = Arc::new(crate::engine::eval::presenter::NoopPresenter);
        engine
            .run_turn(TurnRequest::new("hello"), presenter)
            .await
            .expect("main turn should succeed");

        let history_before = engine.session_manager.active_messages().await.unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();

        let response = engine
            .run_ephemeral_turn(
                EphemeralTurnRequest {
                    prompt: "quick side question".to_string(),
                    max_tokens: Some(128),
                    ..Default::default()
                },
                Some(tx),
            )
            .await
            .expect("ephemeral turn should succeed");

        assert_eq!(response.text, "btw answer");
        assert_eq!(response.usage.map(|u| u.output_tokens), Some(12));

        let mut deltas = Vec::new();
        while let Ok(delta) = rx.try_recv() {
            deltas.push(delta);
        }
        assert_eq!(deltas, vec!["btw ", "answer"]);

        let history_after = engine.session_manager.active_messages().await.unwrap();
        assert_eq!(
            history_before, history_after,
            "ephemeral turn must not mutate session history"
        );

        let requests = model.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].preamble, requests[1].preamble,
            "preamble must match main turn"
        );
        assert_eq!(requests[0].tools, requests[1].tools, "tools must match main turn");
        assert_eq!(
            requests[1].prompt, "",
            "prompt field stays empty for prefix cache alignment"
        );
        assert_eq!(requests[1].max_tokens, Some(128));
        assert_eq!(
            requests[1].messages.last().and_then(ChatMessage::text),
            Some("quick side question")
        );
    }

    #[tokio::test]
    async fn ephemeral_turn_handles_partial_assistant_without_breaking_alternation() {
        let model = Arc::new(MockAdapter::text("side answer"));
        let (engine, _dir) = mock_engine_in_dir("ephemeral-partial", model.clone());
        engine
            .session_manager
            .extend_checkpoint(vec![ChatMessage::user("in-flight user prompt")])
            .await
            .unwrap();

        let resp = engine
            .run_ephemeral_turn(
                EphemeralTurnRequest {
                    prompt: "what is this doing?".to_string(),
                    partial_assistant: Some("Working on it...".to_string()),
                    extra_messages: vec![
                        ChatMessage::user("first btw"),
                        ChatMessage::assistant("first btw answer"),
                    ],
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        assert_eq!(resp.text, "side answer");

        let reqs = model.requests();
        assert_eq!(reqs.len(), 1);
        let roles: Vec<&str> = reqs[0]
            .messages
            .iter()
            .map(|m| match m {
                ChatMessage::System { .. } => "system",
                ChatMessage::User { .. } => "user",
                ChatMessage::Assistant { .. } => "assistant",
            })
            .collect();
        assert_eq!(roles, vec!["user", "assistant", "user", "assistant", "user"]);
    }

    #[derive(Debug)]
    struct HangingStreamModel;

    #[async_trait]
    impl ModelAdapter for HangingStreamModel {
        fn model_name(&self) -> &str {
            "hanging"
        }

        fn provider_name(&self) -> &str {
            "mock"
        }

        async fn stream(&self, _request: ModelCompletionRequest) -> Result<ModelEventStream> {
            let stream = futures::stream::once(async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                Ok(ModelStreamEvent::Text("late".to_string()))
            });
            Ok(Box::pin(stream))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn ephemeral_turn_aborts_cleanly_on_cancellation() {
        let (engine, _dir) = mock_engine_in_dir("ephemeral-cancel", Arc::new(HangingStreamModel));
        let signal = CancellationSignal::default();
        let cancel_handle = signal.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            cancel_handle.cancel();
        });

        let err = engine
            .run_ephemeral_turn(
                EphemeralTurnRequest {
                    prompt: "hello".to_string(),
                    cancellation: signal,
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Cancelled(_)), "expected Cancelled, got {err:?}");
    }

    #[tokio::test]
    async fn ephemeral_turn_errors_clearly_when_model_is_missing() {
        let (mut engine, _dir) = mock_engine_in_dir("ephemeral-no-model", Arc::new(MockAdapter::default()));
        engine.model = None;
        let err = engine
            .run_ephemeral_turn(
                EphemeralTurnRequest {
                    prompt: "hello".to_string(),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Config(_)), "expected Config error, got {err:?}");
    }

    #[test]
    fn build_recap_prompt_anchors_with_title_and_next_todo() {
        let todos = vec![
            TodoItem {
                content: "Done step".to_string(),
                status: TodoStatus::Completed,
            },
            TodoItem {
                content: "Wire ephemeral turn".to_string(),
                status: TodoStatus::InProgress,
            },
            TodoItem {
                content: "Run tests".to_string(),
                status: TodoStatus::Pending,
            },
        ];

        let anchored = build_recap_prompt(Some("Refactor Engine"), Some(&todos));
        assert!(anchored.contains("<recap>"));
        assert!(anchored.contains("</recap>"));
        assert!(anchored.contains("Session title: Refactor Engine"));
        assert!(anchored.contains("Next todo: Wire ephemeral turn"));

        let unanchored = build_recap_prompt(Some("   "), None);
        assert!(!unanchored.contains("Session title:"));
        assert!(!unanchored.contains("Next todo:"));
    }

    #[test]
    fn format_recap_text_sanitizes_and_truncates_safely() {
        assert_eq!(format_recap_text("  <recap>\n  </recap>  "), None);
        assert_eq!(
            format_recap_text("  \"<recap>Working on\n  ephemeral   turns.</recap>\"  "),
            Some("Working on ephemeral turns.".to_string())
        );

        let long = "é".repeat(300);
        let formatted = format_recap_text(&long).expect("non-empty recap");
        assert_eq!(formatted.chars().count(), MAX_RECAP_CHARS);
        assert!(formatted.ends_with('…'));
    }

    #[tokio::test]
    async fn generate_recap_returns_none_on_empty_history_and_formats_non_empty() {
        let model = Arc::new(MockAdapter::text(
            "<recap>Adding ephemeral turn support.\nNext: wire /btw.</recap>",
        ));
        let (engine, _dir) = mock_engine_in_dir("ephemeral-recap", model);

        let empty = engine.generate_recap(CancellationSignal::default()).await.unwrap();
        assert_eq!(empty, None);

        engine
            .session_manager
            .extend_checkpoint(vec![ChatMessage::user("start"), ChatMessage::assistant("started")])
            .await
            .unwrap();
        let recap = engine.generate_recap(CancellationSignal::default()).await.unwrap();
        assert_eq!(
            recap,
            Some("Adding ephemeral turn support. Next: wire /btw.".to_string())
        );
    }
}
