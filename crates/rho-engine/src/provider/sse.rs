//! Shared SSE line framing, stream unfolding, and completion aggregation for
//! provider streaming implementations (OpenAI-compatible, Claude, Antigravity, ChatGPT).

use futures::{Stream, StreamExt, stream};
use rho_harness_core::error::AppError;
use std::pin::Pin;
use std::time::Duration;

use crate::engine::metrics::StructuralUsage;
use crate::provider::adapter::{ModelCompletionResponse, ModelStreamEvent};

pub const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Default)]
pub struct SseLineDecoder {
    buffer: Vec<u8>,
}

impl SseLineDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn decode_raw_lines(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(bytes);
        let mut lines = Vec::new();
        let mut cursor = 0;

        while let Some(rel) = memchr::memchr(b'\n', &self.buffer[cursor..]) {
            let line_end = cursor + rel;
            let line = String::from_utf8_lossy(&self.buffer[cursor..line_end]);
            lines.push(line.trim_end_matches('\r').to_string());
            cursor = line_end + 1;
        }

        if cursor > 0 {
            self.buffer.drain(..cursor);
        }
        lines
    }

    /// Feed byte chunks and extract ready `data:` payloads.
    pub fn decode_lines(&mut self, bytes: &[u8]) -> Vec<String> {
        self.decode_raw_lines(bytes)
            .iter()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .filter(|payload| !payload.is_empty() && *payload != "[DONE]")
            .map(str::to_string)
            .collect()
    }
}

/// Provider-specific incremental SSE parser that transforms wire bytes into domain stream events.
pub trait SseStreamParser: Send + 'static {
    fn feed(&mut self, bytes: &[u8]) -> Vec<Result<ModelStreamEvent, AppError>>;
}

type StreamState<S, P> = (S, P, bool);
type EventBatch = Vec<Result<ModelStreamEvent, AppError>>;

async fn next_stream_batch<B, S, P>(
    (mut bytes, mut parser, finished): StreamState<S, P>,
) -> Option<(EventBatch, StreamState<S, P>)>
where
    B: AsRef<[u8]>,
    S: Stream<Item = reqwest::Result<B>> + Unpin,
    P: SseStreamParser,
{
    if finished {
        return None;
    }
    loop {
        let chunk = match tokio::time::timeout(STREAM_IDLE_TIMEOUT, bytes.next()).await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => return None,
            Err(_) => {
                let err = AppError::Provider("Provider stream went idle without sending data".to_string());
                return Some((vec![Err(err)], (bytes, parser, true)));
            }
        };
        match chunk {
            Ok(data) => {
                let events = parser.feed(data.as_ref());
                if !events.is_empty() {
                    let finished = events
                        .iter()
                        .any(|e| matches!(e, Ok(ModelStreamEvent::Usage(_)) | Err(_)));
                    return Some((events, (bytes, parser, finished)));
                }
            }
            Err(e) => {
                let err = AppError::Provider(format!("Stream transport failed: {e}"));
                return Some((vec![Err(err)], (bytes, parser, true)));
            }
        }
    }
}

/// Unfold a provider's HTTP body into a pinned stream of domain events.
pub fn unfold_sse_stream<S, B, P>(
    bytes: S,
    parser: P,
) -> Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>
where
    S: Stream<Item = reqwest::Result<B>> + Send + Unpin + 'static,
    B: AsRef<[u8]> + Send + 'static,
    P: SseStreamParser,
{
    let events = stream::unfold((bytes, parser, false), next_stream_batch)
        .map(stream::iter)
        .flatten();
    Box::pin(events)
}

/// Map a provider's opened HTTP response (or its `(status, body)` failure) into an event stream.
pub fn open_sse_stream<P: SseStreamParser>(
    opened: Result<reqwest::Response, (Option<u16>, String)>,
    friendly_error: fn(Option<u16>, &str) -> String,
    parser: P,
) -> Result<crate::provider::adapter::ModelEventStream, AppError> {
    let response = opened.map_err(|(status, body)| AppError::Provider(friendly_error(status, &body)))?;
    Ok(unfold_sse_stream(response.bytes_stream(), parser))
}

/// Defines `feed` for a line-oriented SSE parser exposing a `decoder` field and `interpret_line`.
macro_rules! sse_line_feed {
    () => {
        pub fn feed(&mut self, bytes: &[u8]) -> SseEvents {
            let lines = self.decoder.decode_lines(bytes);
            let mut events = Vec::new();
            for line in lines {
                self.interpret_line(&line, &mut events);
            }
            events
        }
    };
}
pub(crate) use sse_line_feed;

/// Aggregate a vector of streaming events into a single unary `ModelCompletionResponse`.
pub fn aggregate_stream_events(
    events: Vec<Result<ModelStreamEvent, AppError>>,
) -> Result<ModelCompletionResponse, AppError> {
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    let mut usage = StructuralUsage::default();
    let mut finish_reason = None;

    for event in events {
        match event? {
            ModelStreamEvent::Text(t) => content.push_str(&t),
            ModelStreamEvent::Reasoning(_) => {}
            ModelStreamEvent::ToolCall(call) => tool_calls.push(call),
            ModelStreamEvent::Usage(u) => usage = usage.merge(&u),
            ModelStreamEvent::Finish(f) => finish_reason = Some(f),
        }
    }

    Ok(ModelCompletionResponse {
        content,
        tool_calls,
        usage,
        finish_reason,
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

    #[test]
    fn aggregate_stream_events_sums_usage_across_events() {
        let usage = |total| {
            Ok(ModelStreamEvent::Usage(StructuralUsage {
                input_tokens: 4,
                total_tokens: total,
                ..Default::default()
            }))
        };
        let response = aggregate_stream_events(vec![usage(10), usage(5)]).unwrap();
        assert_eq!((response.usage.input_tokens, response.usage.total_tokens), (8, 15));
    }

    #[test]
    fn sse_line_decoder_keeps_multibyte_characters_split_across_chunks() {
        let mut decoder = SseLineDecoder::new();
        let bytes = "data: \"é\"\n".as_bytes();
        let split = bytes.iter().position(|b| *b == 0xC3).unwrap() + 1;
        assert!(decoder.decode_raw_lines(&bytes[..split]).is_empty());
        assert_eq!(decoder.decode_raw_lines(&bytes[split..]), vec!["data: \"é\""]);
    }

    #[tokio::test(start_paused = true)]
    async fn unfold_sse_stream_fails_instead_of_hanging_when_body_goes_idle() {
        struct NoEvents;
        impl SseStreamParser for NoEvents {
            fn feed(&mut self, _bytes: &[u8]) -> Vec<Result<ModelStreamEvent, AppError>> {
                Vec::new()
            }
        }

        let idle_body = futures::stream::iter(vec![Ok::<_, reqwest::Error>(b"keepalive".to_vec())])
            .chain(futures::stream::pending());
        let mut events = unfold_sse_stream(idle_body, NoEvents);
        let first = tokio::time::timeout(Duration::from_secs(3600), events.next())
            .await
            .expect("idle stream must error, not hang");
        assert!(matches!(first, Some(Err(AppError::Provider(msg))) if msg.contains("idle")));
    }
}
