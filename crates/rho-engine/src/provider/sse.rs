//! Shared SSE line framing, stream unfolding, and completion aggregation for
//! provider streaming implementations (Claude, Antigravity, ChatGPT).

use futures::{Stream, StreamExt, stream};
use rho_harness_core::error::AppError;
use std::pin::Pin;

use crate::engine::metrics::StructuralUsage;
use crate::provider::adapter::{ModelCompletionResponse, ModelStreamEvent};

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

/// Provider-specific incremental SSE parser that transforms wire bytes into domain stream events.
pub trait SseStreamParser: Send + 'static {
    fn feed(&mut self, bytes: &[u8]) -> Vec<Result<ModelStreamEvent, AppError>>;
}

type StreamState<S, P> = (S, P, bool);

async fn next_stream_batch<B, S, P>(
    (mut byte_stream, mut parser, finished): StreamState<S, P>,
) -> Option<(Vec<Result<ModelStreamEvent, AppError>>, StreamState<S, P>)>
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
                        .any(|e| matches!(e, Ok(ModelStreamEvent::Usage(_)) | Err(_)));
                    return Some((events, (byte_stream, parser, has_term)));
                }
            }
            Err(e) => {
                let err = AppError::Provider(format!("Stream transport failed: {e}"));
                return Some((vec![Err(err)], (byte_stream, parser, true)));
            }
        }
    }
    None
}

/// Unfold an SSE HTTP response into a pinned stream of domain events.
pub fn unfold_sse_stream<P>(
    response: reqwest::Response,
    parser: P,
) -> Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>
where
    P: SseStreamParser,
{
    let event_stream = stream::unfold((response.bytes_stream(), parser, false), next_stream_batch)
        .map(stream::iter)
        .flatten();
    Box::pin(event_stream)
}

/// Aggregate a vector of streaming events into a single unary `ModelCompletionResponse`.
pub fn aggregate_stream_events(
    events: Vec<Result<ModelStreamEvent, AppError>>,
) -> Result<ModelCompletionResponse, AppError> {
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    let mut usage = StructuralUsage::default();

    for event in events {
        match event? {
            ModelStreamEvent::Text(t) => content.push_str(&t),
            ModelStreamEvent::Reasoning(_) => {}
            ModelStreamEvent::ToolCall(call) => tool_calls.push(call),
            ModelStreamEvent::Usage(u) => usage = u,
        }
    }

    Ok(ModelCompletionResponse {
        content,
        tool_calls,
        usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rho_harness_core::model::{ToolCall, ToolFunction};

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
            Ok(ModelStreamEvent::Text("Hello ".to_string())),
            Ok(ModelStreamEvent::Reasoning("thinking".to_string())),
            Ok(ModelStreamEvent::Text("world!".to_string())),
            Ok(ModelStreamEvent::Usage(StructuralUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                ..Default::default()
            })),
        ];

        let response = aggregate_stream_events(events).unwrap();
        assert_eq!(response.content, "Hello world!");
        assert_eq!(response.usage.total_tokens, 15);
    }

    #[test]
    fn aggregate_stream_events_attaches_tool_call_and_usage() {
        let events = vec![
            Ok(ModelStreamEvent::ToolCall(ToolCall::new(
                "call-1",
                ToolFunction::new("bash", serde_json::json!({"command": "ls"})),
            ))),
            Ok(ModelStreamEvent::Usage(StructuralUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                ..Default::default()
            })),
        ];

        let response = aggregate_stream_events(events).unwrap();
        assert_eq!(response.usage.total_tokens, 15);
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].function.name, "bash");
    }

    #[tokio::test]
    async fn test_unfold_sse_stream_pipeline() {
        struct TestParser;
        impl SseStreamParser for TestParser {
            fn feed(&mut self, bytes: &[u8]) -> Vec<Result<ModelStreamEvent, AppError>> {
                if bytes == b"chunk" {
                    vec![Ok(ModelStreamEvent::Text("chunk".to_string()))]
                } else if bytes == b"term" {
                    vec![Ok(ModelStreamEvent::Usage(StructuralUsage {
                        total_tokens: 1,
                        ..Default::default()
                    }))]
                } else {
                    vec![]
                }
            }
        }

        let byte_stream = futures::stream::iter(vec![
            Ok::<_, reqwest::Error>(Vec::from(&b"chunk"[..])),
            Ok::<_, reqwest::Error>(Vec::from(&b"term"[..])),
        ]);
        let state = (Box::pin(byte_stream), TestParser, false);
        let batch = next_stream_batch(state).await;
        assert!(batch.is_some());
        let (events, state) = batch.unwrap();
        assert_eq!(events.len(), 1);
        let batch2 = next_stream_batch(state).await;
        assert!(batch2.is_some());
        let (events2, state2) = batch2.unwrap();
        assert_eq!(events2.len(), 1);
        let batch3 = next_stream_batch(state2).await;
        assert!(batch3.is_none());
    }
}
