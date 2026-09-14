//! Shared SSE line framing, stream unfolding, and completion aggregation for
//! provider streaming implementations (Claude, Antigravity, ChatGPT).

use futures::{Stream, StreamExt, stream};
use rig::completion::{CompletionError, CompletionResponse, FinishReason, Usage};
use rig::message::{AssistantContent, Reasoning, ReasoningContent, Text, ToolCall};
use rig::streaming::{RawStreamingChoice, StreamFinal};
use std::pin::Pin;

/// Incremental SSE byte buffer decoder that extracts clean `data: ...` payload lines.
#[derive(Default)]
pub struct SseLineDecoder {
    buffer: Vec<u8>,
}

impl SseLineDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed byte chunks and extract ready `data:` payloads.
    ///
    /// Strips carriage returns (`\r`), leading `data:`, and skips empty lines or `[DONE]`.
    pub fn decode_lines(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(bytes);
        let mut lines = Vec::new();
        let mut cursor = 0;

        while let Some(rel) = memchr::memchr(b'\n', &self.buffer[cursor..]) {
            let line_end = cursor + rel;
            let line_bytes = &self.buffer[cursor..line_end];
            cursor = line_end + 1;

            let line = String::from_utf8_lossy(line_bytes);
            let trimmed = line.trim_end_matches('\r');
            if let Some(data) = trimmed.strip_prefix("data:") {
                let payload = data.trim();
                if !payload.is_empty() && payload != "[DONE]" {
                    lines.push(payload.to_string());
                }
            }
        }

        if cursor > 0 {
            self.buffer.drain(..cursor);
        }

        lines
    }
}

/// Provider-specific incremental SSE parser that transforms wire bytes into Rig streaming choices.
pub trait SseStreamParser: Send + 'static {
    fn feed(&mut self, bytes: &[u8]) -> Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>>;
}

type StreamState<S, P> = (S, P, bool);

async fn next_stream_batch<B, S, P>(
    (mut byte_stream, mut parser, finished): StreamState<S, P>,
) -> Option<(
    Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>>,
    StreamState<S, P>,
)>
where
    B: AsRef<[u8]>,
    S: Stream<Item = reqwest::Result<B>> + Unpin,
    P: SseStreamParser,
{
    if finished {
        return None;
    }
    while let Some(chunk) = byte_stream.next().await {
        match chunk {
            Ok(bytes) => {
                let events = parser.feed(bytes.as_ref());
                if !events.is_empty() {
                    let has_term = events
                        .iter()
                        .any(|e| matches!(e, Ok(RawStreamingChoice::FinalResponse(_)) | Err(_)));
                    return Some((events, (byte_stream, parser, has_term)));
                }
            }
            Err(e) => {
                let err = CompletionError::ProviderError(format!("Stream transport failed: {e}"));
                return Some((vec![Err(err)], (byte_stream, parser, true)));
            }
        }
    }
    None
}

/// Unfold an SSE HTTP response into a pinned stream of Rig choices.
pub fn unfold_sse_stream<P>(
    response: reqwest::Response,
    parser: P,
) -> Pin<Box<dyn Stream<Item = Result<RawStreamingChoice<StreamFinal>, CompletionError>> + Send>>
where
    P: SseStreamParser,
{
    let event_stream = stream::unfold((response.bytes_stream(), parser, false), next_stream_batch)
        .map(stream::iter)
        .flatten();
    Box::pin(event_stream)
}

fn append_text(choice: &mut Vec<AssistantContent>, text: &str) {
    if let Some(AssistantContent::Text(last)) = choice.last_mut() {
        last.text.push_str(text);
    } else {
        choice.push(AssistantContent::Text(Text::new(text.to_string())));
    }
}

fn append_reasoning_delta(choice: &mut Vec<AssistantContent>, reasoning: String) {
    if let Some(AssistantContent::Reasoning(last)) = choice.last_mut()
        && let Some(ReasoningContent::Text { text, .. }) = last.content.last_mut()
    {
        text.push_str(&reasoning);
    } else {
        choice.push(AssistantContent::Reasoning(Reasoning {
            id: None,
            content: vec![ReasoningContent::Text {
                text: reasoning,
                signature: None,
            }],
        }));
    }
}

fn append_reasoning_end(choice: &mut Vec<AssistantContent>, reasoning: Option<Reasoning>) {
    if let Some(reasoning) = reasoning {
        if let Some(AssistantContent::Reasoning(last)) = choice.last_mut() {
            *last = reasoning;
        } else {
            choice.push(AssistantContent::Reasoning(reasoning));
        }
    }
}

fn apply_choice_event(
    event: RawStreamingChoice<StreamFinal>,
    (choice, usage, finish_reason): (&mut Vec<AssistantContent>, &mut Usage, &mut Option<FinishReason>),
) {
    match event {
        RawStreamingChoice::Message(text) => append_text(choice, &text),
        RawStreamingChoice::Reasoning { content, .. } => {
            choice.push(AssistantContent::Reasoning(Reasoning {
                id: None,
                content: vec![content],
            }));
        }
        RawStreamingChoice::ReasoningDelta { reasoning, .. } => append_reasoning_delta(choice, reasoning),
        RawStreamingChoice::ReasoningEnd { reasoning, .. } => append_reasoning_end(choice, reasoning),
        RawStreamingChoice::ToolCall(call) => choice.push(AssistantContent::ToolCall(ToolCall::from(call))),
        RawStreamingChoice::FinalResponse(final_response) => {
            *usage = final_response.usage;
            *finish_reason = final_response.finish_reason;
        }
        _ => {}
    }
}

/// Aggregate a vector of streaming choices into a single unary `CompletionResponse`.
pub fn aggregate_stream_events(
    events: Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>>,
    provider_name: &'static str,
) -> Result<CompletionResponse, CompletionError> {
    let mut choice = Vec::new();
    let mut usage = Usage::new();
    let mut finish_reason = None;

    for event in events {
        apply_choice_event(event?, (&mut choice, &mut usage, &mut finish_reason));
    }

    let mut response = CompletionResponse::new(choice, usage, provider_name);
    if let Some(finish_reason) = finish_reason {
        response = response.with_finish_reason(finish_reason);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::streaming::{MintKind, RawStreamingToolCall, StreamPartId};

    #[test]
    fn sse_line_decoder_extracts_clean_lines_across_chunks() {
        let mut decoder = SseLineDecoder::new();

        let lines1 = decoder.decode_lines(b"data: {\"a\": 1}\r\ndata: ");
        assert_eq!(lines1, vec!["{\"a\": 1}"]);

        let lines2 = decoder.decode_lines(b"{\"b\": 2}\n\ndata: [DONE]\n");
        assert_eq!(lines2, vec!["{\"b\": 2}"]);
    }

    #[test]
    fn aggregate_stream_events_merges_text_and_reasoning() {
        let events = vec![
            Ok(RawStreamingChoice::Message("Hello ".to_string())),
            Ok(RawStreamingChoice::ReasoningDelta {
                id: StreamPartId::minted(MintKind::Reasoning, 0),
                provider_id: None,
                reasoning: "thinking ".to_string(),
            }),
            Ok(RawStreamingChoice::ReasoningDelta {
                id: StreamPartId::minted(MintKind::Reasoning, 0),
                provider_id: None,
                reasoning: "harder".to_string(),
            }),
            Ok(RawStreamingChoice::Message("world!".to_string())),
            Ok(RawStreamingChoice::FinalResponse(
                StreamFinal::new("test-provider", Usage::new()).with_finish_reason(FinishReason::Stop),
            )),
        ];

        let response = aggregate_stream_events(events, "test-provider").unwrap();
        assert_eq!(response.choice.len(), 3);
        assert!(matches!(&response.choice[0], AssistantContent::Text(t) if t.text == "Hello "));
        let reasoning = match &response.choice[1] {
            AssistantContent::Reasoning(r) => match &r.content[0] {
                ReasoningContent::Text { text, .. } => text.as_str(),
                _ => "",
            },
            _ => "",
        };
        assert_eq!(reasoning, "thinking harder");
        assert!(matches!(&response.choice[2], AssistantContent::Text(t) if t.text == "world!"));
    }

    #[test]
    fn aggregate_stream_events_attaches_tool_call_and_usage() {
        let events = vec![
            Ok(RawStreamingChoice::ToolCall(RawStreamingToolCall::new(
                StreamPartId::wire("call-1"),
                "bash".to_string(),
                serde_json::json!({"command": "ls"}),
            ))),
            Ok(RawStreamingChoice::FinalResponse(
                StreamFinal::new(
                    "test-provider",
                    Usage {
                        input_tokens: 10,
                        output_tokens: 5,
                        total_tokens: 15,
                        reasoning_tokens: 2,
                        ..Usage::new()
                    },
                )
                .with_finish_reason(FinishReason::Stop),
            )),
        ];

        let response = aggregate_stream_events(events, "test-provider").unwrap();
        assert_eq!(response.usage.total_tokens, 15);
        assert_eq!(response.finish_reason(), Some(FinishReason::ToolCalls));
        assert_eq!(response.choice.len(), 1);
        assert!(matches!(&response.choice[0], AssistantContent::ToolCall(tc) if tc.function.name == "bash"));
    }
}
